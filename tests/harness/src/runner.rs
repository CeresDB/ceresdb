use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use anyhow::{Context, Result};
use ceresdb_client_rs::client::Client;
use tokio::{
    fs::{remove_file, File, OpenOptions},
    io::AsyncWriteExt,
};
use walkdir::WalkDir;

use crate::case::TestCase;

/// Input SQL query cases
const TEST_CASE_EXTENSION: &'static str = "sql";
/// Output generated by running queries
const OUTPUT_FILE_EXTENSION: &'static str = "out";
/// Expected result provides with queries
const RESULT_FILE_EXTENSION: &'static str = "result";
/// Tools to compare file
const DIFF_BINARY: &'static str = "diff";

pub struct Runner {
    case_root: String,
    client: Client,
}

impl Runner {
    pub fn new<P: AsRef<Path>>(root_dir: P, client: Client) -> Self {
        Self {
            case_root: root_dir.as_ref().as_os_str().to_str().unwrap().to_owned(),
            client,
        }
    }

    pub async fn run(&self) {
        let case_paths = self.collect_cases();
        let case_count = case_paths.len();
        let mut diff_cases = vec![];
        for path in case_paths {
            let result: Result<()> = try {
                let case = TestCase::from_file(path.with_extension(TEST_CASE_EXTENSION)).await?;
                let output_path = path.with_extension(OUTPUT_FILE_EXTENSION);
                let mut output_file = Self::open_output_file(&output_path).await?;

                let timer = Instant::now();
                case.execute(&self.client, &mut output_file).await?;
                let elapsed = timer.elapsed();

                output_file.flush().await?;
                let is_different = self.compare(&path);
                if !is_different {
                    remove_file(output_path).await?;
                } else {
                    diff_cases.push(path.as_os_str().to_str().unwrap().to_owned());
                }

                println!(
                    "Takes {:?}. Diff: {}. Test case {:?} finished.",
                    elapsed,
                    is_different,
                    case.to_string()
                );
            };

            if result.is_err() {
                println!(
                    "Error: Failed to run test {:?}, {:?}",
                    path.with_extension(""),
                    result
                );
            }
        }

        // print result.
        println!(
            "Run {} finished. {} cases are different.",
            case_count,
            diff_cases.len()
        );
        if !diff_cases.is_empty() {
            println!("Different cases:");
            println!("{:#?}", diff_cases);
        }
    }

    /// Collects all the file in ".sql" extension under the `root` dir. The
    /// returned path doesn't contains ".sql" extension.
    fn collect_cases(&self) -> Vec<PathBuf> {
        WalkDir::new(&self.case_root)
            .into_iter()
            .filter_map(|entry| {
                entry
                    .map_or(None, |entry| Some(entry.path().to_path_buf()))
                    .filter(|path| {
                        path.extension()
                            .map(|ext| ext == TEST_CASE_EXTENSION)
                            .unwrap_or(false)
                    })
            })
            .map(|path| path.with_extension(""))
            .collect()
    }

    async fn open_output_file<P: AsRef<Path>>(path: P) -> Result<File> {
        OpenOptions::default()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .await
            .with_context(|| format!("Cannot open output file at {:?}", path.as_ref()))
    }

    /// Compare files' diff, return true if two files are different
    fn compare<P: AsRef<Path>>(&self, path: P) -> bool {
        let diff = Command::new(DIFF_BINARY)
            .arg(
                path.as_ref()
                    .with_extension(RESULT_FILE_EXTENSION)
                    .as_os_str(),
            )
            .arg(
                path.as_ref()
                    .with_extension(OUTPUT_FILE_EXTENSION)
                    .as_os_str(),
            )
            .output()
            .expect(&format!("Cannot diff over {:?}", path.as_ref()));

        !diff.stdout.is_empty()
    }
}
