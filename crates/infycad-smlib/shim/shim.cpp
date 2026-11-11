// SM_API (C++) を Rust から呼ぶための C インターフェース。
// 例外は境界を越えさせず、戻り値 0 = 成功、正 = SmApiStatus、-1 = 例外。

#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmApiQueries.h>
#include <SmBrep.h>
#include <SmPoly.h>
#include <SmThreadLocalStorage.h>
#include <SmVector3d.h>

#include <cstdlib>
#include <cstring>
#include <vector>

namespace
{
template <typename F>
int Guard(F&& f)
{
    try
    {
        SmApiStatus status = f();
        return status == SM_SUCCESS ? 0 : (static_cast<int>(status) > 0 ? static_cast<int>(status) : -2);
    }
    catch (...)
    {
        return -1;
    }
}

SmVector3d Vec(const double* p)
{
    return SmVector3d(p[0], p[1], p[2]);
}
} // namespace

extern "C"
{
void infy_init()
{
#ifdef _WIN32
    // 上流は DLL 前提で、スレッドローカル領域を DllMain で確保する。静的リンクでは自分で呼ぶ。
    SmThreadLocalStorage::SetupTLS(SM_TLS_PROCESS_ATTACH);
#endif
    SmApiCreateContext();
}

void infy_brep_free(void* brep)
{
    delete static_cast<SmBrep*>(brep);
}

int infy_box(const double* corner, const double* size, void** out)
{
    return Guard([&] {
        SmBrep* result = nullptr;
        SmApiStatus status = SmApiCreateBox(Vec(corner), size[0], size[1], size[2], result);
        *out = result;
        return status;
    });
}

int infy_cylinder(const double* base, double radius, double height, void** out)
{
    return Guard([&] {
        SmBrep* result = nullptr;
        SmApiStatus status = SmApiCreateCylinder(Vec(base), radius, height, result);
        *out = result;
        return status;
    });
}

int infy_sphere(const double* center, double radius, void** out)
{
    return Guard([&] {
        SmBrep* result = nullptr;
        SmApiStatus status = SmApiCreateSphere(Vec(center), radius, result);
        *out = result;
        return status;
    });
}

// op: 0 = 和, 1 = 差 (a - b), 2 = 積。a と b は変更しない（複製に対して演算する）。
int infy_boolean(int op, const void* a, const void* b, void** out)
{
    *out = nullptr;
    SmBrep* copyA = nullptr;
    SmBrep* copyB = nullptr;
    int rc = Guard([&] {
        SmApiStatus status = SmApiBrepCopy(static_cast<const SmBrep*>(a), copyA);
        if (status != SM_SUCCESS)
            return status;
        status = SmApiBrepCopy(static_cast<const SmBrep*>(b), copyB);
        if (status != SM_SUCCESS)
            return status;
        SmBrep* result = nullptr;
        switch (op)
        {
        case 0: status = SmApiBooleanUnion(copyA, copyB, result); break;
        case 1: status = SmApiBooleanDifference(copyA, copyB, result); break;
        default: status = SmApiBooleanIntersection(copyA, copyB, result); break;
        }
        if (status == SM_SUCCESS)
        {
            // 成功時は copyB をカーネルが解放し、result は copyA の別名。
            *out = result;
            copyA = nullptr;
            copyB = nullptr;
        }
        return status;
    });
    // 失敗時は両方とも残っている。例外時はどこまで進んだか分からないので解放しない。
    if (rc > 0 || rc == -2)
    {
        delete copyA;
        delete copyB;
    }
    return rc;
}

int infy_volume(const void* brep, double accuracy, double* out)
{
    return Guard([&] { return SmApiBrepComputeVolume(static_cast<const SmBrep*>(brep), accuracy, *out); });
}

// 三角形ごとに 9 個の座標 (*positions) と 3 個の面法線 (*normals) を返す。infy_free で解放する。
int infy_tessellate(const void* brep, double chord, double** positions, double** normals, size_t* triangles)
{
    *positions = nullptr;
    *normals = nullptr;
    *triangles = 0;
    return Guard([&] {
        SmTessellationParams params;
        params.dChordHeightTol = chord;
        // 既定の 25 度では曲面が粗く見えるので、表示用に細かくする。
        params.dCurveAngleTolDeg = 10.0;
        params.dSurfaceAngleTolDeg = 10.0;
        SmPolyBrep* poly = nullptr;
        SmApiStatus status = SmApiTessellate(static_cast<SmBrep*>(const_cast<void*>(brep)), poly, params);
        if (status != SM_SUCCESS || poly == nullptr)
            return status != SM_SUCCESS ? status : SM_ERR;

        std::vector<double> pos, nrm;
        SmTArray<SmPolyFace*> faces;
        poly->GetPolyFaces(faces);
        for (ULONG f = 0; f < faces.GetSize(); ++f)
        {
            SmTArray<SmPolyVertex*> verts;
            faces[f]->GetPolyVertices(verts);
            const SmVector3d n = faces[f]->GetNormal();
            // 三角形のはずだが、多角形が来ても扇形に分割する。
            for (ULONG i = 2; i < verts.GetSize(); ++i)
            {
                const SmPolyVertex* tri[3] = { verts[0], verts[i - 1], verts[i] };
                for (const SmPolyVertex* v : tri)
                {
                    const SmPoint3d p = v->GetPoint();
                    pos.insert(pos.end(), { p[0], p[1], p[2] });
                }
                nrm.insert(nrm.end(), { n[0], n[1], n[2] });
            }
        }
        delete poly;

        const size_t count = nrm.size() / 3;
        if (count > 0)
        {
            *positions = static_cast<double*>(std::malloc(pos.size() * sizeof(double)));
            *normals = static_cast<double*>(std::malloc(nrm.size() * sizeof(double)));
            std::memcpy(*positions, pos.data(), pos.size() * sizeof(double));
            std::memcpy(*normals, nrm.data(), nrm.size() * sizeof(double));
        }
        *triangles = count;
        return SM_SUCCESS;
    });
}

void infy_free(void* p)
{
    std::free(p);
}
}
