use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFileOptions {
    pub file_path: PathBuf,
}

pub fn parse_memory_file_options(
    arguments: &[String],
    startup_directory: &Path,
) -> Result<MemoryFileOptions, String> {
    if startup_directory.as_os_str().is_empty() {
        return Err("Startup directory must not be empty.".to_string());
    }

    let mut configured_file_path: Option<PathBuf> = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--file" => {
                if configured_file_path.is_some() {
                    return Err("The '--file' option may only be specified once.".to_string());
                }

                let value = arguments.get(index + 1).ok_or_else(|| {
                    "Missing value for '--file'. Expected '--file <path>'.".to_string()
                })?;

                if value.trim().is_empty() {
                    return Err("The '--file' value must not be empty or whitespace.".to_string());
                }

                configured_file_path = Some(PathBuf::from(value));
                index += 2;
            }
            argument => {
                return Err(format!(
                    "Unknown argument '{argument}'. Expected '--file <path>'."
                ));
            }
        }
    }

    let file_path = configured_file_path
        .unwrap_or_else(|| startup_directory.join(".engram").join("memory.json"));

    Ok(MemoryFileOptions { file_path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_workspace_default_when_file_argument_is_not_provided() {
        let startup_directory = PathBuf::from("/workspace");

        let options = parse_memory_file_options(&[], &startup_directory).unwrap();

        assert_eq!(
            options.file_path,
            PathBuf::from("/workspace/.engram/memory.json")
        );
    }

    #[test]
    fn uses_explicit_file_path_when_file_argument_is_provided() {
        let startup_directory = PathBuf::from("/workspace");
        let options = parse_memory_file_options(
            &["--file".to_string(), "some/path.json".to_string()],
            &startup_directory,
        )
        .unwrap();

        assert_eq!(options.file_path, PathBuf::from("some/path.json"));
    }

    #[test]
    fn throws_clear_error_when_file_value_is_missing() {
        let startup_directory = PathBuf::from("/workspace");
        let error =
            parse_memory_file_options(&["--file".to_string()], &startup_directory).unwrap_err();

        assert!(error.contains("Missing value for '--file'"));
    }

    #[test]
    fn throws_clear_error_for_unknown_arguments() {
        let startup_directory = PathBuf::from("/workspace");
        let error =
            parse_memory_file_options(&["--wat".to_string()], &startup_directory).unwrap_err();

        assert!(error.contains("Unknown argument '--wat'"));
    }
}
