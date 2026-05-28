trust_attrs::strict!{}

use super::file_tree::{File, FileTree, FileType};
use std::collections::HashMap;
use std::fs;

// Type alias works around R0042's generic-type comma splitting:
// `&HashMap<usize, usize>` would otherwise be parsed as 2 params.
// See RT-29 followup ticket.
type ChildCount = HashMap<usize, usize>;

#[derive(Debug, Clone, PartialEq)]
enum PrefixSegment {
    ShapeL, // "└── "
    ShapeT, // "├── "
    ShapeI, // "│   "
    Empty,  // "    "
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormattedEntry {
    pub name: String,
    pub path: String,
    pub prefix: String,
    pub link: Option<String>,
}

fn make_prefix(tree: &FileTree, file: &File, format_history: &ChildCount) -> String {
    let mut segments = Vec::new();
    let mut current = file;
    if let Some(ancestor) = tree.get_parent(file) {
        let count = format_history.get(&ancestor.id).unwrap_or(&0);
        if *count >= ancestor.children_count() - 1 {
            segments.push(PrefixSegment::ShapeL);
        } else {
            segments.push(PrefixSegment::ShapeT);
        }
        current = ancestor;
    }

    while let Some(ancestor) = tree.get_parent(current) {
        let count = format_history.get(&ancestor.id).unwrap_or(&0);
        if *count == ancestor.children_count() {
            segments.push(PrefixSegment::Empty);
        } else {
            segments.push(PrefixSegment::ShapeI);
        }
        current = ancestor;
    }

    segments.reverse();
    segments.iter().fold(String::new(), |s, seg| {
        s + match seg {
            PrefixSegment::ShapeL => "└── ",
            PrefixSegment::ShapeT => "├── ",
            PrefixSegment::ShapeI => "│   ",
            PrefixSegment::Empty => "    ",
        }
    })
}

fn format_file(
    tree: &FileTree,
    file: &File,
    format_history: &mut ChildCount,
    result: &mut Vec<FormattedEntry>,
    make_absolute: bool,
) {
    let prefix = make_prefix(tree: tree, file: file, format_history: format_history);
    let path = if make_absolute {
        // R0001: canonicalize can fail on broken symlinks or missing files;
        // fall back to the original relative path rather than panic.
        match fs::canonicalize(&file.path) {
            Ok(p) => p.display().to_string(),
            Err(_) => file.path.clone(),
        }
    } else {
        file.path.clone()
    };

    result.push(FormattedEntry {
        name: file.display_name.clone(),
        path,
        prefix,
        link: file.link(),
    });

    if let Some(parent) = tree.get_parent(file) {
        if let Some(&n) = format_history.get(&parent.id) {
            format_history.insert(key: parent.id, value: n + 1);
        }
    }

    if let FileType::Directory = file.file_type {
        format_history.insert(key: file.id, value: 0);
    }

    if let Some(children) = file.children() {
        for child_id in children.values() {
            format_file(
                tree: tree,
                file: tree.get(*child_id),
                format_history: format_history,
                result: result,
                make_absolute: make_absolute,
            );
        }
    }
}

/// R0012: replaced `make_absolute: bool` with a named enum so call sites
/// like `format_paths(p, c, true)` cannot silently flip the flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    Relative,
    Absolute,
}

pub fn format_paths(
    root_path: &str,
    children: Vec<(String, FileType)>,
    path_style: PathStyle,
) -> Vec<FormattedEntry> {
    let make_absolute = matches!(path_style, PathStyle::Absolute);
    let mut history = ChildCount::new();
    let mut result = Vec::new();
    // R0042: `FileTree::new(root_path, children)` left positional — the local
    // registry's only `new` is `SerializableTreeNode::from_tree`; cross-file
    // resolution isn't implemented. See RT-29 followups.
    match FileTree::new(root_path, children) {
        Some(tree) => {
            let root = tree.get_root();
            format_file(
                tree: &tree,
                file: root,
                format_history: &mut history,
                result: &mut result,
                make_absolute: make_absolute,
            );
            result
        }
        None => Vec::new(),
    }
}

#[cfg(test)]
mod test {
    use super::FormattedEntry;
    use crate::file_tree::FileType;
    use std::path;

    #[test]
    fn formatting_works() {
        let formatted = super::format_paths(
            root_path: ".",
            children: vec![
                ("a".to_string(), FileType::File),
                (format!("b{}c", path::MAIN_SEPARATOR), FileType::File),
            ],
            path_style: super::PathStyle::Relative,
        );

        let bc_path = format!("b{}c", path::MAIN_SEPARATOR);
        let b_path = format!(".{}b", path::MAIN_SEPARATOR);
        let variant0 = vec![
            FormattedEntry {
                name: ".".to_string(),
                path: ".".to_string(),
                prefix: String::new(),
                link: None,
            },
            FormattedEntry {
                name: "a".to_string(),
                path: "a".to_string(),
                prefix: "├── ".to_string(),
                link: None,
            },
            FormattedEntry {
                name: "b".to_string(),
                path: b_path.clone(),
                prefix: "└── ".to_string(),
                link: None,
            },
            FormattedEntry {
                name: "c".to_string(),
                path: bc_path.clone(),
                prefix: "    └── ".to_string(),
                link: None,
            },
        ];

        let variant1 = vec![
            FormattedEntry {
                name: ".".to_string(),
                path: ".".to_string(),
                prefix: String::new(),
                link: None,
            },
            FormattedEntry {
                name: "b".to_string(),
                path: b_path.clone(),
                prefix: "├── ".to_string(),
                link: None,
            },
            FormattedEntry {
                name: "c".to_string(),
                path: bc_path.clone(),
                prefix: "│   └── ".to_string(),
                link: None,
            },
            FormattedEntry {
                name: "a".to_string(),
                path: "a".to_string(),
                prefix: "└── ".to_string(),
                link: None,
            },
        ];

        assert!(formatted == variant0 || formatted == variant1);
    }
}
