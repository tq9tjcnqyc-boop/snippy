mod config;
mod engine;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut cli_config: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            cli_config = Some(args[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }

    let cmd = args.get(1).map(String::as_str).unwrap_or("run");
    match cmd {
        "run" | "start" => {
            let (cfg, path) = config::load(cli_config.as_deref());
            let enabled = cfg.snippets.iter().filter(|s| s.enabled).count();
            println!(
                "snippy — 监听中 (配置: {})，{} 条缩写已启用，前缀 `{}`",
                path.display(),
                enabled,
                cfg.prefix
            );
            let engine = engine::Engine::new(cfg, path);
            engine.spawn_aux();
            engine.listen_main();
        }
        "list" | "show" => {
            let (cfg, path) = config::load(cli_config.as_deref());
            println!("配置文件: {}", path.display());
            for s in cfg.snippets.iter().filter(|s| s.enabled) {
                let value = s.expand.replace('\n', " ");
                println!(
                    "  {:<8} -> {}   {}",
                    s.trigger,
                    value,
                    s.description
                        .as_deref()
                        .map(|d| format!("({d})"))
                        .unwrap_or_default()
                );
            }
        }
        "add" => {
            let Some(trigger) = args.get(2).cloned() else {
                eprintln!("用法: snippy add <触发词> <扩展文本>");
                std::process::exit(2);
            };
            let Some(expand) = args.get(3).cloned() else {
                eprintln!("用法: snippy add <触发词> <扩展文本>");
                std::process::exit(2);
            };
            cmd_add(cli_config.as_deref(), trigger, expand);
        }
        "remove" => {
            let Some(trigger) = args.get(2).cloned() else {
                eprintln!("用法: snippy remove <触发词>");
                std::process::exit(2);
            };
            cmd_remove(cli_config.as_deref(), trigger);
        }
        "daemon" => cmd_daemon(cli_config.as_deref()),
        "stop" => cmd_stop(),
        "status" => cmd_status(),
        "help" | "--help" | "-h" => print_help(),
        _ => print_help(),
    }
}

fn cmd_add(cli: Option<&str>, trigger: String, expand: String) {
    let (mut cfg, path) = config::load(cli);
    if trigger.trim().is_empty() || trigger.contains(char::is_whitespace) {
        eprintln!("触发词不能为空或包含空格。");
        std::process::exit(2);
    }
    if !trigger.is_ascii() {
        eprintln!("触发词仅支持 ASCII（英文/数字），中文触发词已不再支持。");
        std::process::exit(2);
    }
    if cfg.snippets.iter().any(|s| s.trigger == trigger) {
        println!("触发词 \"{trigger}\" 已存在，跳过。");
        return;
    }
    cfg.snippets.push(config::Snippet {
        trigger: trigger.clone(),
        expand: expand.clone(),
        word: true,
        description: None,
        enabled: true,
    });
    if let Err(e) = config::save(&cfg, &path) {
        eprintln!("保存失败: {e}");
        std::process::exit(1);
    }
    println!("已添加: {trigger} -> {expand}");
    println!("配置文件: {}", path.display());
    println!("运行中的进程会自动重载；若未启动，请先 run。");
}

fn cmd_remove(cli: Option<&str>, trigger: String) {
    let (mut cfg, path) = config::load(cli);
    let before = cfg.snippets.len();
    cfg.snippets.retain(|s| s.trigger != trigger);
    if cfg.snippets.len() == before {
        println!("未找到触发词 \"{trigger}\"。");
        return;
    }
    if let Err(e) = config::save(&cfg, &path) {
        eprintln!("保存失败: {e}");
        std::process::exit(1);
    }
    println!("已删除: {trigger}");
}

fn pid_file() -> PathBuf {
    config::default_config_dir().join("snippy.pid")
}

fn log_file() -> PathBuf {
    config::default_config_dir().join("snippy.log")
}

fn read_pid() -> Option<i32> {
    fs::read_to_string(pid_file()).ok()?.trim().parse().ok()
}

#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_alive(_pid: i32) -> bool {
    true
}

fn cmd_daemon(cli: Option<&str>) {
    let path = config::resolve_path(cli);
    if let Some(pid) = read_pid() {
        if pid_alive(pid) {
            println!("snippy 已在后台运行 (pid {pid})");
            return;
        }
    }

    let mut cmd = Command::new(std::env::current_exe().unwrap_or_default());
    cmd.arg("run").arg("--config").arg(&path).stdin(Stdio::null());
    match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
    {
        Ok(f) => {
            if let Ok(c) = f.try_clone() {
                cmd.stdout(Stdio::from(c));
            }
            cmd.stderr(Stdio::from(f));
        }
        Err(_) => {
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x00000008 | 0x00000200);
    }

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id() as i32;
            let _ = fs::write(pid_file(), pid.to_string());
            println!("snippy 已在后台启动 (pid {pid})");
            println!("日志: {}", log_file().display());
        }
        Err(e) => {
            eprintln!("启动后台失败: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_stop() {
    match read_pid() {
        Some(pid) => {
            if pid_alive(pid) {
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
                #[cfg(windows)]
                {
                    let _ = Command::new("taskkill")
                        .args(["/F", "/PID", &pid.to_string()])
                        .spawn();
                }
                println!("snippy 已停止 (pid {pid})");
            } else {
                println!("snippy 未在运行");
            }
            let _ = fs::remove_file(pid_file());
        }
        None => println!("snippy 未在运行"),
    }
}

fn cmd_status() {
    match read_pid() {
        Some(pid) if pid_alive(pid) => println!("snippy 运行中 (pid {pid})"),
        _ => println!("snippy 未运行"),
    }
}

fn print_help() {
    println!(
        "snippy — 跨平台文本扩展\n\
\n\
用法:\n\
  snippy daemon                   后台启动并常驻 (推荐)\n\
  snippy run [--config <path>]    前台启动监听 (Ctrl+C 停止)\n\
  snippy stop                     停止后台进程\n\
  snippy status                   查看后台是否在运行\n\
  snippy list                    列出已启用的缩写\n\
  snippy add <触发词> <文本>     新增一条缩写\n\
  snippy remove <触发词>         删除一条缩写\n\
  snippy --help                  显示帮助\n\
\n\
配置: ~/.config/snippy/snippy.json\n\
首次运行会自动生成默认配置。\n\
改动配置会在运行中自动生效（热重载）。\n\
触发前缀默认是 `/`，输入 `/cs` 才会扩展；改成 \"prefix\": \"\" 可取消前缀。\n\
\n\
注意: macOS 需要在 系统设置→安全性与隐私→隐私 里，把本进程分别加入\n\
      「辅助功能」(用于注入) 和「输入监控」(用于监听)，再完全退出重开。"
    );
}
