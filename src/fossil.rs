use std::{
    collections::HashMap,
    env, fmt,
    path::PathBuf,
    str::FromStr,
    time::{Duration, SystemTime},
    vec::Vec,
};

use async_walkdir::{Filtering, WalkDir};
use color_eyre::eyre::{OptionExt, eyre};
use futures_lite::StreamExt;
use git2::{
    Cred, FetchOptions, RemoteCallbacks, Repository,
    build::{CheckoutBuilder, RepoBuilder},
};
use serde::{Deserialize, Serialize, de::Visitor};
use tokio::fs;
use tracing::{debug, info, warn};
use yaml_rust2::YamlLoader;

use crate::util;

type Res<T> = color_eyre::Result<T>;

#[derive(Clone)]
pub struct RepoHandler {
    access_token: String,
    clone_path: PathBuf,
    pub latest_commit: CommitRef,
}

#[derive(Debug, Clone)]
pub struct CommitRef {
    pub shorthand: String,
    pub commit_time_utc: SystemTime,
}

impl RepoHandler {
    pub fn init() -> Res<Self> {
        const REPO_URL: &str = "https://git.cutie.zone/Lyssieth/irzean-writings";

        let access_token = env::var("IRZEAN_ACCESS_TOKEN")?;
        let clone_path: PathBuf = env::var("IRZEAN_CLONE_PATH")?.into();

        let repo = if clone_path.exists() {
            info!("`irzean-writings` opened from {clone_path:?}");

            Repository::open(&clone_path)?
        } else {
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(|_url, username_from_url, _allowed_types| {
                Cred::userpass_plaintext(username_from_url.unwrap_or("irzean"), &access_token)
            });

            let mut fo = FetchOptions::new();
            fo.remote_callbacks(callbacks);

            let mut builder = RepoBuilder::new();
            builder.fetch_options(fo);

            info!("`irzean-writings` cloned to {clone_path:?}");

            builder.clone(REPO_URL, &clone_path)?
        };

        let head = repo.head()?;

        let shorthand = head.shorthand().ok_or_eyre("invalid utf8 for shorthand")?;
        let commit = repo.find_commit(head.target().ok_or_eyre("invalid target")?)?;

        let time = commit.time();

        let commit_time_utc =
            SystemTime::UNIX_EPOCH + Duration::from_secs(time.seconds().try_into()?);

        info!("Latest commit ({shorthand}) is at {commit_time_utc:?}");

        Ok(Self {
            access_token,
            clone_path,
            latest_commit: CommitRef {
                shorthand: shorthand.to_string(),
                commit_time_utc,
            },
        })
    }

    pub async fn file_list(&self) -> Res<Vec<Writing>> {
        let mut walk = WalkDir::new(&self.clone_path).filter(async |v| {
            v.path()
                .extension()
                .map(|ext| !ext.eq_ignore_ascii_case("md"))
                .map_or(Filtering::Ignore, |b| {
                    if b {
                        Filtering::Ignore
                    } else {
                        Filtering::Continue
                    }
                })
        });

        let mut list = Vec::new();

        let root_path = &self.clone_path;

        while let Some(entry) = walk.try_next().await? {
            let path = entry.path();
            let relative = path.strip_prefix(root_path)?;

            let content = fs::read_to_string(&path).await?;

            if content.starts_with("---") {
                debug!("Found preamble in {relative:?}");
            } else {
                warn!("No preamble in {relative:?}, skipping");
                continue;
            }

            let Some(ending_hr) = content[3..].find("---") else {
                warn!("Could not find ending hr in {relative:?}, skipping");
                continue;
            };
            let ending_hr = ending_hr + 3; // account for the offset

            let preamble = &content[3..ending_hr];

            let docs = YamlLoader::load_from_str(preamble)?;
            let Some(doc) = docs.first() else {
                warn!("No YAML documents loadable in {relative:?}, skipping");
                continue;
            };

            let Some(title) = doc["title"].as_str() else {
                warn!("No title found in {relative:?}, skipping");
                continue;
            };
            let Some(date_authored) = doc["date"].as_str() else {
                warn!("no date found in {relative:?}, skipping");
                continue;
            };
            let date_authored: DateTriple = date_authored.parse()?;

            let tags = doc["tags"].as_vec().map_or_else(Vec::new, |tags| {
                let mut out = Vec::new();

                for tag in tags {
                    let Some(tag) = tag.as_str() else {
                        warn!(?tag, "Invalid tag, skipping");
                        continue;
                    };

                    out.push(tag.to_string());
                }

                out
            });

            let is_nsfw = doc["nsfw"].as_bool().unwrap_or_default();
            let is_hidden = doc["hidden"].as_bool().unwrap_or_default();

            let description = doc["description"].as_str().map(ToString::to_string);

            let previous = doc["previous"].as_str().map(ToString::to_string);
            let next = doc["next"].as_str().map(ToString::to_string);

            list.push(Writing {
                rel_path: relative.to_path_buf(),
                title: title.to_string(),
                date_authored,
                content,
                description,
                tags,
                is_nsfw,
                is_hidden,
                previous,
                next,
            });
        }

        list.sort_by_cached_key(|v| v.date_authored);
        list.reverse();

        Ok(list)
    }

    pub async fn get_writing(&self, relative_path: &str) -> Res<Writing> {
        let file_list = self.file_list().await?;

        file_list
            .into_iter()
            .find(|v| {
                let rel_path = util::slugify_path(&v.rel_path);

                rel_path.eq(relative_path)
            })
            .ok_or_eyre("Couldn't find a suitable Writing")
    }

    pub async fn tag_list(&self) -> Res<HashMap<String, u64>> {
        let mut tags = HashMap::new();

        let writings = self.file_list().await?;
        for writing in writings.iter().filter(|v| !v.is_hidden) {
            for tag in &writing.tags {
                tags.entry(tag.clone()).and_modify(|v| *v += 1).or_insert(1);
            }
        }

        tags.insert(
            "nsfw".to_string(),
            writings
                .iter()
                .filter(|v| v.is_nsfw && !v.is_hidden)
                .count() as u64,
        );
        tags.insert(
            "sfw".to_string(),
            writings
                .iter()
                .filter(|v| !v.is_nsfw && !v.is_hidden)
                .count() as u64,
        );

        Ok(tags)
    }

    pub fn update(&mut self) -> Res<()> {
        let repo = self.get_repo()?;
        {
            let mut fo = self.fetch_options();

            repo.find_remote("origin")?
                .fetch(&["main"], Some(&mut fo), None)?;
        }

        let fetch_head = repo.find_reference("FETCH_HEAD")?;
        let commit = repo.reference_to_annotated_commit(&fetch_head)?;
        let commit = repo.find_object(commit.id(), None)?;

        let mut checkout = CheckoutBuilder::new();
        checkout.force();

        repo.checkout_tree(&commit, Some(&mut checkout))?;

        let head = repo.head()?;

        let shorthand = head.shorthand().ok_or_eyre("invalid utf8 for shorthand")?;
        let commit = repo.find_commit(head.target().ok_or_eyre("invalid target")?)?;

        let time = commit.time();

        let commit_time_utc =
            SystemTime::UNIX_EPOCH + Duration::from_secs(time.seconds().try_into()?);

        self.latest_commit = CommitRef {
            commit_time_utc,
            shorthand: shorthand.to_string(),
        };

        Ok(())
    }

    fn fetch_options(&self) -> FetchOptions {
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, _allowed_types| {
            Cred::userpass_plaintext(username_from_url.unwrap_or("irzean"), &self.access_token)
        });

        let mut fo = FetchOptions::new();
        fo.remote_callbacks(callbacks);

        fo
    }

    fn get_repo(&self) -> Res<Repository> {
        Ok(Repository::open(&self.clone_path)?)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Writing {
    pub rel_path: PathBuf,
    pub title: String,
    pub date_authored: DateTriple,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub is_nsfw: bool,
    pub is_hidden: bool,
    pub content: String,
    pub previous: Option<String>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTriple {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl FromStr for DateTriple {
    type Err = color_eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let split: Vec<_> = s.split('-').collect();

        if split.len() != 3 {
            return Err(eyre!("date triple is not a triple"));
        }

        let year = split[0].parse::<u16>()?;
        let month = split[1].parse::<u8>()?;
        let day = split[2].parse::<u8>()?;

        Ok(Self { year, month, day })
    }
}

impl fmt::Display for DateTriple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:0>4}-{:0>2}-{:0>2}", self.year, self.month, self.day)
    }
}

impl<'de> Deserialize<'de> for DateTriple {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(DateTripleVisitor)
    }
}

impl Serialize for DateTriple {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!(
            "{:0>4}-{:0>2}-{:0>2}",
            self.year, self.month, self.day
        ))
    }
}

pub struct DateTripleVisitor;

impl Visitor<'_> for DateTripleVisitor {
    type Value = DateTriple;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a date triple in the format `yyyy-mm-dd`")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_borrowed_str(v)
    }

    fn visit_borrowed_str<E>(self, v: &'_ str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let split: Vec<_> = v.split('-').collect();

        if split.len() != 3 {
            return Err(serde::de::Error::custom("date triple is not a triple"));
        }

        let year = split[0]
            .parse::<u16>()
            .map_err(|e| serde::de::Error::custom(format!("invalid int for year: {e}")))?;
        let month = split[1]
            .parse::<u8>()
            .map_err(|e| serde::de::Error::custom(format!("invalid int for month: {e}")))?;
        let day = split[2]
            .parse::<u8>()
            .map_err(|e| serde::de::Error::custom(format!("invalid int for day: {e}")))?;

        Ok(DateTriple { year, month, day })
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_borrowed_str(&v)
    }
}
