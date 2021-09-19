use crate::config::Config;
use crate::cookies;

use anyhow::Result;

pub async fn download(config: &Config) -> Result<()> {
    let cookies = cookies::read_cookies(&config.cookies_file).unwrap();
    downloader::download(&config, cookies).await?;

    Ok(())
}

mod downloader {
    use crate::config::{CafeConfig, Config};

    use anyhow::Result;
    use indicatif::{ProgressBar, ProgressStyle};
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
    struct CafeApiResponse {
        addfiles: CafeAddFiles,
        #[serde(rename = "imageList")]
        image_list: Vec<String>,
        #[serde(rename = "plainTextOfName")]
        name: String,
        #[serde(rename = "regDttm")]
        date: String,
        #[serde(rename = "subcontent")]
        content: String,
    }

    #[derive(Deserialize, Debug)]
    struct CafeAddFiles {
        addfile: Vec<CafeFile>,
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
            let download_path = if let Some(p) = &cafe.download_path {
                Path::new(p)
            } else {
                Path::new("cafe")
            }
            .join(cafe_name)
            .join(cafe_board);

            fs::create_dir_all(&download_path)?;

            let first_id = Downloader::get_first_id(&download_path);
            let mut missing_id_cnt = 0;
            for id in first_id.. {
                let api_url = format!("http://api.m.cafe.daum.net/mcafe/api/v1/hybrid/{}/{}/{}?ref=&isSimple=false&installedVersion=3.15.1", &cafe_name, &cafe_board, id);
                let resp = self.client_auth.get(api_url).send().await?;
                let resp = match resp.json::<CafeApiResponse>().await {
                    Ok(j) => j,
                    Err(_) => {
                        missing_id_cnt += 1;
                        if missing_id_cnt >= 5 {
                            break;
                        }
                        continue;
                    }
                };

                let prefix = sanitize_filename::sanitize(format!(
                    "{}_{}_{}_{:04}_{}",
                    &resp.date[..8],
                    cafe_name,
                    cafe_board,
                    id,
                    Downloader::truncate_str_to_length(&resp.name, 100),
                ));

                let post_download_path = download_path.join(&prefix);
                self.download_post(&resp, &post_download_path, &prefix)
                    .await?;
                missing_id_cnt = 0;
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
                    .unwrap_or(1)
                    + 1;
            }

            1
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

            // Progress bar
            let pb = ProgressBar::new(post.addfiles.addfile.len() as u64);
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
            futures::stream::iter(post.addfiles.addfile.iter().map(|image| {
                let real_idx = post
                    .image_list
                    .iter()
                    .position(|u| get_url_basename(u) == get_url_basename(image.downurl.as_str()));
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
                        "{}_attachimg{:03}.{}",
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
            {
                let mut file = File::create(
                    dir.path()
                        .join(format!("{}.txt", prefix.as_ref().to_string_lossy())),
                )?;
                file.write_all(post.content.as_bytes())?;
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
