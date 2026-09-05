use crate::config::Config;
use enigo::{Direction, Enigo, Keyboard, Key, Settings};
use rdev::{listen, Event, EventType, Key as RKey};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

struct SnippetSpec {
    trigger: Vec<char>,
    expand: String,
    word: bool,
}

enum InjectCmd {
    Text { count: usize, text: String },
}

pub struct Engine {
    snippets: Arc<Mutex<Vec<SnippetSpec>>>,
    buffer: Arc<Mutex<VecDeque<char>>>,
    expanding: Arc<AtomicBool>,
    tx: Sender<InjectCmd>,
    config_path: PathBuf,
    prefix: String,
}

impl Engine {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let prefix = config.prefix.clone();
        let snippets = Arc::new(Mutex::new(build_specs(config)));
        let buffer = Arc::new(Mutex::new(VecDeque::new()));
        let (tx, rx) = channel();
        let expanding = Arc::new(AtomicBool::new(false));
        let worker_expanding = expanding.clone();
        thread::spawn(move || inject_worker(rx, worker_expanding));
        Self {
            snippets,
            buffer,
            expanding,
            tx,
            config_path,
            prefix,
        }
    }

    /// 在后台线程启动「配置热重载」。注意：全局监听必须在主线程跑（macOS）。
    pub fn spawn_aux(&self) {
        let s = self.snippets.clone();
        let path = self.config_path.clone();
        thread::spawn(move || reload_loop(path, s));
    }

    /// 在主线程运行全局键盘监听（阻塞）。必须由主线程调用，否则 macOS 会因 TSM 崩溃。
    pub fn listen_main(self) {
        let Engine {
            snippets,
            buffer,
            expanding,
            tx,
            config_path: _,
            prefix,
        } = self;
        let prefix_chars: Vec<char> = prefix.chars().collect();
        let mut shift = false;
        let callback = move |ev: Event| {
            if expanding.load(Ordering::SeqCst) {
                return;
            }
            match ev.event_type {
                EventType::KeyPress(key) => {
                    if is_shift_key(key) {
                        shift = true;
                        return;
                    }
                    if key == RKey::Backspace {
                        if let Ok(mut buf) = buffer.lock() {
                            buf.pop_back();
                        }
                        return;
                    }
                    let Some(ch) = key_to_char(key, shift) else {
                        return;
                    };
                    let mut buf = match buffer.lock() {
                        Ok(b) => b,
                        Err(_) => return,
                    };
                    buf.push_back(ch);
                    let snap = match snippets.lock() {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let max_len = snap
                        .iter()
                        .map(|s| s.trigger.len() + prefix_chars.len())
                        .max()
                        .unwrap_or(8)
                        .max(64);
                    while buf.len() > max_len {
                        buf.pop_front();
                    }
                    if let Some((spec, deletable)) = find_match(&snap, &buf, &prefix_chars) {
                        let expand = spec.expand.clone();
                        buf.clear();
                        drop(buf);
                        let _ = tx.send(InjectCmd::Text {
                            count: deletable,
                            text: expand,
                        });
                        // 触发瞬间清掉 shift：若触发词是用 Shift 打出来的（如大写/符号），
                        // 松开事件会在 expanding 期间被吞掉，不清会导致后续 shift 卡 true。
                        shift = false;
                        expanding.store(true, Ordering::SeqCst);
                    }
                }
                EventType::KeyRelease(key) => {
                    if is_shift_key(key) {
                        shift = false;
                    }
                }
                _ => {}
            }
        };
        if let Err(e) = listen(callback) {
            eprintln!("监听器启动失败: {e:?}");
            eprintln!("macOS 请在 系统设置→安全性与隐私→隐私 里，把本进程加入「辅助功能」和「输入监控」。");
        }
    }
}

fn build_specs(config: Config) -> Vec<SnippetSpec> {
    config
        .snippets
        .into_iter()
        // 只保留 ASCII 触发词：中文/非 ASCII 触发词已被移除，无法通过按键路径匹配。
        .filter(|s| s.enabled && s.trigger.is_ascii())
        .map(|s| SnippetSpec {
            trigger: s.trigger.chars().collect(),
            expand: s.expand,
            word: s.word,
        })
        .collect()
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

fn reload_loop(path: PathBuf, snippets: Arc<Mutex<Vec<SnippetSpec>>>) {
    let mut last = mtime(&path);
    loop {
        thread::sleep(Duration::from_secs(1));
        let current = mtime(&path);
        if current == last {
            continue;
        }
        last = current;
        match crate::config::try_read(&path) {
            Some(cfg) => {
                let new = build_specs(cfg);
                *snippets.lock().unwrap() = new;
                println!("snippy: 配置已重载");
            }
            None => eprintln!("snippy: 配置解析失败，保留旧配置"),
        }
    }
}

fn is_shift_key(key: RKey) -> bool {
    matches!(key, RKey::ShiftLeft | RKey::ShiftRight)
}

fn key_to_char(key: RKey, shift: bool) -> Option<char> {
    use RKey::*;
    let base = match key {
        KeyA => 'a',
        KeyB => 'b',
        KeyC => 'c',
        KeyD => 'd',
        KeyE => 'e',
        KeyF => 'f',
        KeyG => 'g',
        KeyH => 'h',
        KeyI => 'i',
        KeyJ => 'j',
        KeyK => 'k',
        KeyL => 'l',
        KeyM => 'm',
        KeyN => 'n',
        KeyO => 'o',
        KeyP => 'p',
        KeyQ => 'q',
        KeyR => 'r',
        KeyS => 's',
        KeyT => 't',
        KeyU => 'u',
        KeyV => 'v',
        KeyW => 'w',
        KeyX => 'x',
        KeyY => 'y',
        KeyZ => 'z',
        Num0 => return Some(if shift { ')' } else { '0' }),
        Num1 => return Some(if shift { '!' } else { '1' }),
        Num2 => return Some(if shift { '@' } else { '2' }),
        Num3 => return Some(if shift { '#' } else { '3' }),
        Num4 => return Some(if shift { '$' } else { '4' }),
        Num5 => return Some(if shift { '%' } else { '5' }),
        Num6 => return Some(if shift { '^' } else { '6' }),
        Num7 => return Some(if shift { '&' } else { '7' }),
        Num8 => return Some(if shift { '*' } else { '8' }),
        Num9 => return Some(if shift { '(' } else { '9' }),
        SemiColon => return Some(if shift { ':' } else { ';' }),
        Quote => return Some(if shift { '"' } else { '\'' }),
        Comma => return Some(if shift { '<' } else { ',' }),
        Dot => return Some(if shift { '>' } else { '.' }),
        Slash => return Some(if shift { '?' } else { '/' }),
        Minus => return Some(if shift { '_' } else { '-' }),
        Equal => return Some(if shift { '+' } else { '=' }),
        LeftBracket => return Some(if shift { '{' } else { '[' }),
        RightBracket => return Some(if shift { '}' } else { ']' }),
        BackQuote => return Some(if shift { '~' } else { '`' }),
        BackSlash => return Some(if shift { '|' } else { '\\' }),
        Space => return Some(' '),
        _ => return None,
    };
    Some(if shift {
        base.to_ascii_uppercase()
    } else {
        base
    })
}

fn ends_with(buf: &VecDeque<char>, chars: &[char]) -> bool {
    if buf.len() < chars.len() {
        return false;
    }
    let start = buf.len() - chars.len();
    let tail: Vec<char> = buf.iter().skip(start).copied().collect();
    tail.as_slice() == chars
}

fn find_match<'a>(
    snippets: &'a [SnippetSpec],
    buf: &VecDeque<char>,
    prefix_chars: &[char],
) -> Option<(&'a SnippetSpec, usize)> {
    for spec in snippets {
        let tchars = &spec.trigger;
        if tchars.is_empty() {
            continue;
        }
        let total = prefix_chars.len() + tchars.len();
        if buf.len() < total || !ends_with(buf, tchars) {
            continue;
        }
        if !prefix_chars.is_empty() {
            let tstart = buf.len() - tchars.len();
            let pstart = tstart - prefix_chars.len();
            let got: Vec<char> = buf
                .iter()
                .skip(pstart)
                .take(prefix_chars.len())
                .copied()
                .collect();
            if got.as_slice() != prefix_chars {
                continue;
            }
        } else if spec.word {
            let tstart = buf.len() - tchars.len();
            if tstart > 0 && is_word_char(buf[tstart - 1]) {
                continue;
            }
        }
        return Some((spec, total));
    }
    None
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn inject_worker(rx: Receiver<InjectCmd>, expanding: Arc<AtomicBool>) {
    let Ok(mut enigo) = Enigo::new(&Settings::default()) else {
        eprintln!("无法初始化输入注入器 (enigo)，请检查辅助功能/输入监控权限。");
        return;
    };
    for cmd in rx {
        let InjectCmd::Text { count, text } = cmd;
        remove_chars(&mut enigo, count);
        let _ = enigo.text(&text);
        thread::sleep(Duration::from_millis(60));
        expanding.store(false, Ordering::SeqCst);
    }
}

/// 连续回退 `count` 个字符，删掉用户已输入的前缀+触发词。
fn remove_chars(enigo: &mut Enigo, count: usize) {
    for _ in 0..count {
        let _ = enigo.key(Key::Backspace, Direction::Click);
        thread::sleep(Duration::from_millis(12));
    }
    thread::sleep(Duration::from_millis(15));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(trigger: &str, word: bool) -> SnippetSpec {
        SnippetSpec {
            trigger: trigger.chars().collect(),
            expand: "X".into(),
            word,
        }
    }

    fn buf(chars: &str) -> VecDeque<char> {
        chars.chars().collect()
    }

    #[test]
    fn matches_exact_trigger_no_prefix() {
        let specs = [spec("sr", true)];
        let p: Vec<char> = vec![];
        assert_eq!(find_match(&specs, &buf("sr"), &p).map(|(_, n)| n), Some(2));
    }

    #[test]
    fn no_match_inside_word_when_word_boundary() {
        let specs = [spec("sr", true)];
        let p: Vec<char> = vec![];
        assert!(find_match(&specs, &buf("asr"), &p).is_none());
    }

    #[test]
    fn prefix_requires_colon_and_counts_it() {
        let specs = [spec("sr", true)];
        let p: Vec<char> = vec![':'];
        assert_eq!(find_match(&specs, &buf(":sr"), &p).map(|(_, n)| n), Some(3));
        assert!(find_match(&specs, &buf("sr"), &p).is_none());
    }

    #[test]
    fn prefix_boundary_via_prefix() {
        let specs = [spec("sr", true)];
        let p: Vec<char> = vec![':'];
        assert_eq!(
            find_match(&specs, &buf("ab:csr??"), &p).map(|(_, n)| n),
            None
        );
        assert_eq!(find_match(&specs, &buf(":sr"), &p).map(|(_, n)| n), Some(3));
    }

    #[test]
    fn word_false_matches_inside_word() {
        let specs = [spec("sr", false)];
        let p: Vec<char> = vec![];
        assert_eq!(find_match(&specs, &buf("asr"), &p).map(|(_, n)| n), Some(2));
    }

    #[test]
    fn chinese_is_not_word_char() {
        assert!(!is_word_char('好'));
        assert!(is_word_char('_'));
    }

    #[test]
    fn shift_maps_colon_and_uppercase() {
        assert_eq!(key_to_char(RKey::SemiColon, true), Some(':'));
        assert_eq!(key_to_char(RKey::SemiColon, false), Some(';'));
        assert_eq!(key_to_char(RKey::KeyS, true), Some('S'));
        assert_eq!(key_to_char(RKey::KeyS, false), Some('s'));
        assert_eq!(key_to_char(RKey::Num1, true), Some('!'));
    }

    #[test]
    fn slash_requires_unshifted_to_match_prefix() {
        // 前缀 `/` 在未按 Shift 时是 '/', 按了 Shift 变成 '?'
        assert_eq!(key_to_char(RKey::Slash, false), Some('/'));
        assert_eq!(key_to_char(RKey::Slash, true), Some('?'));
    }

    #[test]
    fn space_maps_to_ascii_space() {
        assert_eq!(key_to_char(RKey::Space, false), Some(' '));
    }
}
