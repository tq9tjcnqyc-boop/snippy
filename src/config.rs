use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    /// 触发缩写，例如 "sr"（仅支持 ASCII 小写字母/数字；中文触发词已移除）
    pub trigger: String,
    /// 扩展出来的文本
    pub expand: String,
    /// 是否只在「词边界」触发（避免出现在英文单词中间）
    #[serde(default = "default_true")]
    pub word: bool,
    /// 描述，仅用于 list 命令展示
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 触发前缀（默认 `:`）。非空时，只有输入「前缀+触发词」才会触发。
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default)]
    pub snippets: Vec<Snippet>,
}

fn default_prefix() -> String {
    "/".into()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            prefix: default_prefix(),
            snippets: vec![
                Snippet {
                    trigger: "sr".into(),
                    expand: "生日快乐".into(),
                    word: true,
                    description: Some("生日快乐".into()),
                    enabled: true,
                },
                Snippet {
                    trigger: "tmw".into(),
                    expand: "本周周报模板：\n- 完成了：\n- 进行中：\n- 下一步：".into(),
                    word: true,
                    description: Some("周报模板".into()),
                    enabled: true,
                },
                Snippet {
                    trigger: "ema".into(),
                    expand: "ageha@example.com".into(),
                    word: true,
                    description: Some("邮箱".into()),
                    enabled: true,
                },
                Snippet {
                    trigger: "bj".into(),
                    expand: "北京".into(),
                    word: true,
                    description: Some("城市 - 中文".into()),
                    enabled: true,
                },
            ],
        }
    }
}

fn home() -> PathBuf {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into())
        .into()
}

pub fn default_config_dir() -> PathBuf {
    let home = home();
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        home.join(".config").join("snippy")
    }
    #[cfg(target_os = "windows")]
    {
        home.join("AppData").join("Roaming").join("snippy")
    }
}

pub fn default_config_path() -> PathBuf {
    default_config_dir().join("snippy.json")
}

pub fn resolve_path(cli: Option<&str>) -> PathBuf {
    if let Some(p) = cli {
        return PathBuf::from(p);
    }
    if let Ok(p) = env::var("SNIPPY_CONFIG") {
        return PathBuf::from(p);
    }
    let cwd = env::current_dir().unwrap_or_default().join("snippy.json");
    if cwd.exists() {
        return cwd;
    }
    default_config_path()
}

/// 加载配置；找不到或解析失败时写一份默认配置，但**不会**覆盖已存在的坏文件。
pub fn load(cli: Option<&str>) -> (Config, PathBuf) {
    let path = resolve_path(cli);
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<Config>(&text) {
            return (cfg, path);
        }
        eprintln!(
            "警告: 配置文件解析失败 ({}), 使用内置默认配置，请检查 JSON。",
            path.display()
        );
    }
    let default = Config::default();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&default) {
            let _ = fs::write(&path, json);
        }
    }
    (default, path)
}

/// 直接从已知路径读取并解析；文件不存在/解析失败返回 None（用于热重载，不覆盖旧配置）。
pub fn try_read(path: &std::path::Path) -> Option<Config> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(config: &Config, path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, json)
}
