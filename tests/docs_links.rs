use std::path::{Path, PathBuf};

#[test]
fn local_markdown_links_resolve() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut markdown = Vec::new();
    collect_markdown(root, root, &mut markdown);

    let mut broken = Vec::new();
    for file in markdown {
        let contents = std::fs::read_to_string(&file).unwrap();
        for (line_index, line) in contents.lines().enumerate() {
            let mut remaining = line;
            while let Some(link_start) = remaining.find("](") {
                remaining = &remaining[link_start + 2..];
                let Some(link_end) = remaining.find(')') else {
                    break;
                };
                let destination = remaining[..link_end]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches(['<', '>']);
                remaining = &remaining[link_end + 1..];

                if destination.is_empty()
                    || destination.starts_with('#')
                    || destination.contains("://")
                    || destination.starts_with("mailto:")
                {
                    continue;
                }

                let path = destination.split('#').next().unwrap();
                let resolved = file.parent().unwrap().join(path);
                if !resolved.exists() {
                    broken.push(format!(
                        "{}:{}: {destination}",
                        file.strip_prefix(root).unwrap().display(),
                        line_index + 1
                    ));
                }
            }
        }
    }

    assert!(
        broken.is_empty(),
        "broken local Markdown links:\n{}",
        broken.join("\n")
    );
}

fn collect_markdown(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if path == root.join(".git") || path == root.join("target") {
                continue;
            }
            collect_markdown(root, &path, files);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            files.push(path);
        }
    }
}
