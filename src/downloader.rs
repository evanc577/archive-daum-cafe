use crate::config::Config;
use crate::cookies::Auth;

use anyhow::Result;

pub async fn download(config: &Config) -> Result<()> {
    let cookies = Auth::new()?.get_cookies(&config.cookies_file).await?;
    downloader::download(&config, cookies).await?;

    Ok(())
}

mod downloader {
    use crate::config::{CafeConfig, Config};
    use crate::error::DownloaderError;

    use anyhow::Result;
    use indicatif::{ProgressBar, ProgressStyle};
    use lazy_static::lazy_static;
    use serde::Deserialize;
    use std::fs::{self, File};
    use std::io::prelude::*;
    use std::path::Path;

    pub async fn download(config: &Config, cookies: String) -> Result<()> {
        let downloader = Downloader::new(&config, cookies);
        downloader.download_all().await
    }

    struct Downloader<'a> {
        client_auth: reqwest::Client,
        client: reqwest::Client,
        config: &'a Config,
    }

    #[derive(Deserialize, Debug)]
    struct CafeBoardArticles {
        #[serde(rename = "article")]
        articles: Vec<CafeArticle>,
    }

    #[derive(Deserialize, Debug)]
    struct CafeArticle {
        dataid: usize,
        #[serde(rename = "fldid")]
        board: String,
    }

    #[derive(Deserialize, Debug)]
    struct CafeApiResponse {
        addfiles: Option<CafeAddFiles>,
        #[serde(rename = "imageList")]
        image_list: Option<Vec<String>>,
        #[serde(rename = "plainTextOfName")]
        name: Option<String>,
        #[serde(rename = "regDttm")]
        date: Option<String>,
        #[serde(rename = "subcontent")]
        content: Option<String>,
        #[serde(rename = "exceptionCode")]
        exception: Option<String>,
    }

    #[derive(Deserialize, Debug)]
    struct CafeAddFiles {
        addfile: Vec<CafeFile>,
    }

    impl CafeAddFiles {
        fn new() -> Self {
            Self {
                addfile: Vec::new(),
            }
        }
    }

    #[derive(Deserialize, Debug)]
    struct CafeFile {
        downurl: String,
        filetype: String,
    }

    impl<'a> Downloader<'a> {
        fn new(config: &Config, cookies: String) -> Downloader {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(reqwest::header::COOKIE, cookies.parse().unwrap());

            let client_auth = reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .unwrap();
            let client = reqwest::Client::new();
            Downloader {
                client_auth,
                client,
                config,
            }
        }

        async fn download_all(&self) -> Result<()> {
            for cafe in &self.config.cafe {
                self.download_cafe(cafe.0.as_str(), &cafe.1).await?;
            }

            Ok(())
        }

        async fn download_cafe(&self, cafe_name: &str, cafe: &CafeConfig) -> Result<()> {
            for board in &cafe.boards {
                self.download_board(cafe_name, board.as_str(), cafe).await?;
            }

            Ok(())
        }

        async fn download_board(
            &self,
            cafe_name: &str,
            cafe_board: &str,
            cafe: &CafeConfig,
        ) -> Result<()> {
            println!("Checking cafe: {} board: {}", cafe_name, cafe_board);

            // Generate download path
            let download_path = if let Some(p) = &cafe.download_path {
                Path::new(p)
            } else {
                Path::new("cafe")
            }
            .join(cafe_name)
            .join(cafe_board);

            fs::create_dir_all(&download_path)?;

            // Get first ID to start downloading
            let first_id = Downloader::get_first_id(&download_path);
            let latest_id = self.get_latest_id(cafe_name, cafe_board).await?;

            // Download newer posts
            for id in first_id..=latest_id {
                // Query Daum API
                let api_url = format!("http://api.m.cafe.daum.net/mcafe/api/v1/hybrid/{}/{}/{}?ref=&isSimple=false&installedVersion=3.15.1", &cafe_name, &cafe_board, id);
                let resp = self
                    .client_auth
                    .get(api_url)
                    .send()
                    .await?
                    .json::<CafeApiResponse>()
                    .await?;

                // Check API response
                if let Some(exception) = &resp.exception {
                    match exception.as_ref() {
                        "MCAFE_NOT_AUTHENTICATED" => {
                            return Err(DownloaderError::NotAuthenticatedException)?
                        }
                        "MCAFE_BBS_BULLETIN_READ_DELALREADY" => continue,
                        err => return Err(DownloaderError::APIException(err.into()))?,
                    }
                }

                // Generate prefix
                let date = resp
                    .date
                    .as_ref()
                    .ok_or(DownloaderError::APIDateMissing)?
                    .as_str();
                let name = resp
                    .name
                    .as_ref()
                    .ok_or(DownloaderError::APINameMissing)?
                    .as_str();
                let prefix = sanitize_filename::sanitize(format!(
                    "{}_{}_{}_{:04}_{}",
                    &date[..8],
                    cafe_name,
                    cafe_board,
                    id,
                    Downloader::truncate_str_to_length(name, 100),
                ));

                // Download post
                let post_download_path = download_path.join(&prefix);
                self.download_post(&resp, &post_download_path, &prefix)
                    .await?;
            }

            Ok(())
        }

        fn get_first_id(path: &impl AsRef<Path>) -> usize {
            if !path.as_ref().is_dir() {
                return 1;
            }

            let dir = fs::read_dir(path.as_ref());
            if let Ok(dir) = dir {
                return dir
                    .filter_map(|d| {
                        d.ok()?
                            .path()
                            .file_name()?
                            .to_string_lossy()
                            .split('_')
                            .nth(3)?
                            .parse::<usize>()
                            .ok()
                    })
                    .max()
                    .unwrap_or(0)
                    + 1;
            }

            1
        }

        async fn get_latest_id(&self, cafe_name: &str, board: &str) -> Result<usize> {
            let latest_id = self.client
                .get(format!(
                    "https://api.m.cafe.daum.net/mcafe/api/v2/articles/{}/{}",
                    cafe_name, board
                ))
                .send()
                .await?
                .json::<CafeBoardArticles>()
                .await?
                .articles
                .iter()
                .filter_map(|a| match a.board == board {
                    true => Some(a.dataid),
                    false => None,
                })
                .max()
                .ok_or(DownloaderError::APILatestArticleException)?;

            Ok(latest_id)
        }

        fn truncate_str_to_length(input: &str, max_length: usize) -> String {
            use unicode_segmentation::UnicodeSegmentation;

            let graphemes: Vec<_> = UnicodeSegmentation::graphemes(input, true).collect();

            let mut output = "".to_owned();
            for g in graphemes {
                if output.len() + g.len() > max_length {
                    break;
                }
                output.push_str(g);
            }

            output
        }

        async fn download_post(
            &self,
            post: &'a CafeApiResponse,
            path: impl AsRef<Path>,
            prefix: impl AsRef<Path>,
        ) -> Result<()> {
            use futures::stream::StreamExt;
            use tempfile::tempdir;

            println!("Downloading {}", &prefix.as_ref().to_string_lossy());

            lazy_static! {
                static ref EMPTY_ADDFILES: CafeAddFiles = CafeAddFiles::new();
            }

            let addfiles = match &post.addfiles {
                Some(a) => a,
                None => &EMPTY_ADDFILES,
            };

            // Progress bar
            let pb = ProgressBar::new(addfiles.addfile.len() as u64);
            let sty = ProgressStyle::default_bar()
                .template("[{wide_bar}] {pos:>3}/{len:3}")
                .progress_chars("=> ");
            pb.set_style(sty);

            // Create temp directory
            let dir = tempdir()?;

            let get_url_basename =
                |u: &'a str| -> &'a str { u.split('/').into_iter().rev().next().unwrap() };

            // Download all images
            let mut attach_idx: usize = 0;
            futures::stream::iter(addfiles.addfile.iter().map(|image| {
                let real_idx = match &post.image_list {
                    Some(image_list) => image_list.iter().position(|u| {
                        get_url_basename(u) == get_url_basename(image.downurl.as_str())
                    }),
                    None => None,
                };
                let filename = if let Some(idx) = real_idx {
                    format!(
                        "{}_img{:03}.{}",
                        prefix.as_ref().to_string_lossy(),
                        idx + 1,
                        &image.filetype
                    )
                } else {
                    attach_idx += 1;
                    format!(
                        "{}_attach{:03}.{}",
                        prefix.as_ref().to_string_lossy(),
                        attach_idx,
                        &image.filetype
                    )
                };
                let download_path = dir.path().join(filename);
                self.download_image(image.downurl.as_str(), download_path, &pb)
            }))
            .buffer_unordered(self.config.max_connections)
            .collect::<Vec<_>>()
            .await;

            // Write content to text file
            if let Some(content) = &post.content {
                let mut file = File::create(
                    dir.path()
                        .join(format!("{}.txt", prefix.as_ref().to_string_lossy())),
                )?;
                file.write_all(content.as_bytes())?;
            }

            // Move temp directory to final location
            let options = fs_extra::dir::CopyOptions::new();
            fs_extra::dir::copy(&dir.path(), &path.as_ref().parent().unwrap(), &options).unwrap();
            fs::rename(
                path.as_ref()
                    .parent()
                    .unwrap()
                    .join(dir.path().file_name().unwrap()),
                &path,
            )
            .unwrap();

            pb.finish_and_clear();
            Ok(())
        }

        async fn download_image(
            &self,
            url: &str,
            path: impl AsRef<Path>,
            pb: &ProgressBar,
        ) -> Result<()> {
            let body = self.client.get(url).send().await?.bytes().await?;
            let mut buffer = File::create(path)?;
            buffer.write_all(&body)?;
            pb.inc(1);
            Ok(())
        }
    }
}
