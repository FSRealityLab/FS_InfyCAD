//! AutoCAD 互換のコマンド名と短縮名（acad.pgp 相当）。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    Line,
    Polyline,
    Circle,
    Arc,
    Rectangle,
    Move,
    Copy,
    Rotate,
    Mirror,
    Erase,
    Trim,
    Extend,
    Offset,
    Fillet,
    Undo,
    Redo,
    Zoom,
    Pan,
    Orbit,
    Ucs,
    Box,
    Cylinder,
    Sphere,
    Extrude,
    Revolve,
    Union,
    Subtract,
    Intersect,
    FilletEdge,
}

/// (コマンド, 正式名, 短縮名)。短縮名は AutoCAD の既定に合わせる。
const TABLE: &[(Command, &str, &[&str])] = &[
    (Command::Line, "LINE", &["L"]),
    (Command::Polyline, "PLINE", &["PL"]),
    (Command::Circle, "CIRCLE", &["C"]),
    (Command::Arc, "ARC", &["A"]),
    (Command::Rectangle, "RECTANG", &["REC", "RECTANGLE"]),
    (Command::Move, "MOVE", &["M"]),
    (Command::Copy, "COPY", &["CO", "CP"]),
    (Command::Rotate, "ROTATE", &["RO"]),
    (Command::Mirror, "MIRROR", &["MI"]),
    (Command::Erase, "ERASE", &["E", "DELETE"]),
    (Command::Trim, "TRIM", &["TR"]),
    (Command::Extend, "EXTEND", &["EX"]),
    (Command::Offset, "OFFSET", &["O"]),
    (Command::Fillet, "FILLET", &["F"]),
    (Command::Undo, "UNDO", &["U"]),
    (Command::Redo, "REDO", &[]),
    (Command::Zoom, "ZOOM", &["Z"]),
    (Command::Pan, "PAN", &["P"]),
    (Command::Orbit, "3DORBIT", &["3DO", "ORBIT"]),
    (Command::Ucs, "UCS", &[]),
    (Command::Box, "BOX", &[]),
    (Command::Cylinder, "CYLINDER", &["CYL"]),
    (Command::Sphere, "SPHERE", &[]),
    (Command::Extrude, "EXTRUDE", &["EXT"]),
    (Command::Revolve, "REVOLVE", &["REV"]),
    (Command::Union, "UNION", &["UNI"]),
    (Command::Subtract, "SUBTRACT", &["SU"]),
    (Command::Intersect, "INTERSECT", &["IN"]),
    (Command::FilletEdge, "FILLETEDGE", &[]),
];

impl Command {
    /// 正式名または短縮名から引く。大文字小文字は区別せず、先頭の `_` は無視する。
    pub fn lookup(name: &str) -> Option<Command> {
        let name = name.trim().trim_start_matches('_').to_ascii_uppercase();
        TABLE
            .iter()
            .find(|(_, full, aliases)| *full == name || aliases.contains(&name.as_str()))
            .map(|(cmd, _, _)| *cmd)
    }

    pub fn name(self) -> &'static str {
        TABLE
            .iter()
            .find(|(cmd, _, _)| *cmd == self)
            .map(|(_, full, _)| *full)
            .expect("every Command has a TABLE entry")
    }

    /// 入力途中の文字列で始まる正式名（オートコンプリート用）。
    pub fn completions(prefix: &str) -> Vec<&'static str> {
        let prefix = prefix.trim().to_ascii_uppercase();
        if prefix.is_empty() {
            return Vec::new();
        }
        TABLE
            .iter()
            .map(|(_, full, _)| *full)
            .filter(|full| full.starts_with(&prefix))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_names_and_aliases() {
        assert_eq!(Command::lookup("l"), Some(Command::Line));
        assert_eq!(Command::lookup("_LINE"), Some(Command::Line));
        assert_eq!(Command::lookup(" ext "), Some(Command::Extrude));
        assert_eq!(Command::lookup("EX"), Some(Command::Extend));
        assert_eq!(Command::lookup("nope"), None);
    }

    #[test]
    fn names_and_aliases_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (cmd, full, aliases) in TABLE {
            assert_eq!(cmd.name(), *full);
            for n in std::iter::once(full).chain(aliases.iter()) {
                assert!(seen.insert(*n), "duplicate command name {n}");
            }
        }
    }

    #[test]
    fn completes_by_prefix() {
        assert_eq!(Command::completions("ex"), vec!["EXTEND", "EXTRUDE"]);
        assert!(Command::completions("").is_empty());
    }
}
