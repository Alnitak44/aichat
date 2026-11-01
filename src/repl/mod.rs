mod completer;
mod highlighter;
mod prompt;

use self::completer::ReplCompleter;
use self::highlighter::ReplHighlighter;
use self::prompt::ReplPrompt;

use crate::client::{call_chat_completions, call_chat_completions_streaming};
use crate::config::{
    macro_execute, AgentVariables, AssertState, Config, GlobalConfig, Input, LastMessage,
    StateFlags,
};
use crate::render::render_error;
use crate::utils::{
    abortable_run_with_spinner, create_abort_signal, dimmed_text, set_text, temp_file, AbortSignal,
};

use anyhow::{bail, Context, Result};
use crossterm::cursor::SetCursorStyle;
use fancy_regex::Regex;
use reedline::CursorConfig;
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    ColumnarMenu, EditCommand, EditMode, Emacs, KeyCode, KeyModifiers, Keybindings, Reedline,
    ReedlineEvent, ReedlineMenu, ValidationResult, Validator, Vi,
};
use reedline::{MenuBuilder, Signal};
use std::sync::LazyLock;
use std::{env, process};

const MENU_NAME: &str = "completion_menu";

static REPL_COMMANDS: LazyLock<[ReplCommand; 36]> = LazyLock::new(|| {
    [
        ReplCommand::new(".help", "显示帮助", AssertState::pass()),
        ReplCommand::new(".info", "显示系统信息", AssertState::pass()),
        ReplCommand::new(
            ".edit config",
            "编辑配置文件",
            AssertState::False(StateFlags::AGENT),
        ),
        ReplCommand::new(".model", "切换语言模型", AssertState::pass()),
        ReplCommand::new(
            ".prompt",
            "使用提示词临时设置角色",
            AssertState::False(StateFlags::SESSION | StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".role",
            "创建或切换角色",
            AssertState::False(StateFlags::SESSION | StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".info role",
            "显示角色信息",
            AssertState::True(StateFlags::ROLE),
        ),
        ReplCommand::new(
            ".edit role",
            "编辑当前角色",
            AssertState::TrueFalse(StateFlags::ROLE, StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".save role",
            "将当前角色保存到文件",
            AssertState::TrueFalse(
                StateFlags::ROLE,
                StateFlags::SESSION_EMPTY | StateFlags::SESSION,
            ),
        ),
        ReplCommand::new(
            ".exit role",
            "退出当前角色",
            AssertState::TrueFalse(StateFlags::ROLE, StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".session",
            "创建或切换会话",
            AssertState::False(StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".empty session",
            "清空会话消息",
            AssertState::True(StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".compress session",
            "压缩会话消息",
            AssertState::True(StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".info session",
            "显示会话信息",
            AssertState::True(StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".edit session",
            "编辑当前会话",
            AssertState::True(StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".save session",
            "将当前会话保存到文件",
            AssertState::True(StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(
            ".exit session",
            "退出当前会话",
            AssertState::True(StateFlags::SESSION_EMPTY | StateFlags::SESSION),
        ),
        ReplCommand::new(".agent", "使用智能体", AssertState::bare()),
        ReplCommand::new(
            ".starter",
            "使用对话开场白",
            AssertState::True(StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".edit agent-config",
            "编辑智能体配置文件",
            AssertState::True(StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".info agent",
            "显示智能体信息",
            AssertState::True(StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".exit agent",
            "退出智能体",
            AssertState::True(StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".rag",
            "初始化或访问 RAG 知识库",
            AssertState::False(StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".edit rag-docs",
            "向现有 RAG 知识库添加或移除文档",
            AssertState::TrueFalse(StateFlags::RAG, StateFlags::AGENT),
        ),
        ReplCommand::new(
            ".rebuild rag",
            "根据文档变更重建 RAG 知识库",
            AssertState::True(StateFlags::RAG),
        ),
        ReplCommand::new(
            ".sources rag",
            "显示上次回答引用的来源",
            AssertState::True(StateFlags::RAG),
        ),
        ReplCommand::new(
            ".info rag",
            "显示 RAG 知识库信息",
            AssertState::True(StateFlags::RAG),
        ),
        ReplCommand::new(
            ".exit rag",
            "退出 RAG 知识库",
            AssertState::TrueFalse(StateFlags::RAG, StateFlags::AGENT),
        ),
        ReplCommand::new(".macro", "执行宏", AssertState::pass()),
        ReplCommand::new(
            ".file",
            "附加文件、目录、URL 或命令输出",
            AssertState::pass(),
        ),
        ReplCommand::new(
            ".continue",
            "继续上一次回答",
            AssertState::pass(),
        ),
        ReplCommand::new(
            ".regenerate",
            "重新生成上一次回答",
            AssertState::pass(),
        ),
        ReplCommand::new(".copy", "复制上一次回答", AssertState::pass()),
        ReplCommand::new(".set", "修改运行时设置", AssertState::pass()),
        ReplCommand::new(
            ".delete",
            "删除角色、会话、RAG 知识库或智能体",
            AssertState::pass(),
        ),
        ReplCommand::new(".exit", "退出交互模式", AssertState::pass()),
    ]
});
static COMMAND_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\.\S*)\s*").unwrap());
static MULTILINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^\s*:::\s*(.*)\s*:::\s*$").unwrap());

pub struct Repl {
    config: GlobalConfig,
    editor: Reedline,
    prompt: ReplPrompt,
    abort_signal: AbortSignal,
}

impl Repl {
    pub fn init(config: &GlobalConfig) -> Result<Self> {
        let editor = Self::create_editor(config)?;

        let prompt = ReplPrompt::new(config);
        let abort_signal = create_abort_signal();

        Ok(Self {
            config: config.clone(),
            editor,
            prompt,
            abort_signal,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        if AssertState::False(StateFlags::AGENT | StateFlags::RAG)
            .assert(self.config.read().state())
        {
            print!(
                r#"Welcome to {} {}
输入 ".help" 查看帮助。
"#,
                env!("CARGO_CRATE_NAME"),
                env!("CARGO_PKG_VERSION"),
            )
        }

        loop {
            if self.abort_signal.aborted_ctrld() {
                break;
            }
            let sig = self.editor.read_line(&self.prompt);
            match sig {
                Ok(Signal::Success(line)) => {
                    self.abort_signal.reset();
                    match run_repl_command(&self.config, self.abort_signal.clone(), &line).await {
                        Ok(exit) => {
                            if exit {
                                break;
                            }
                        }
                        Err(err) => {
                            render_error(err);
                            println!()
                        }
                    }
                }
                Ok(Signal::CtrlC) => {
                    self.abort_signal.set_ctrlc();
                    println!("(To exit, press Ctrl+D or enter \".exit\")\n");
                }
                Ok(Signal::CtrlD) => {
                    self.abort_signal.set_ctrld();
                    break;
                }
                _ => {}
            }
        }
        self.config.write().exit_session()?;
        Ok(())
    }

    fn create_editor(config: &GlobalConfig) -> Result<Reedline> {
        let completer = ReplCompleter::new(config);
        let highlighter = ReplHighlighter::new(config);
        let menu = Self::create_menu();
        let edit_mode = Self::create_edit_mode(config);
        let cursor_config = CursorConfig {
            vi_insert: Some(SetCursorStyle::BlinkingBar),
            vi_normal: Some(SetCursorStyle::SteadyBlock),
            emacs: None,
        };
        let mut editor = Reedline::create()
            .with_completer(Box::new(completer))
            .with_highlighter(Box::new(highlighter))
            .with_menu(menu)
            .with_edit_mode(edit_mode)
            .with_cursor_config(cursor_config)
            .with_quick_completions(true)
            .with_partial_completions(true)
            .use_bracketed_paste(true)
            .with_validator(Box::new(ReplValidator))
            .with_ansi_colors(true);

        if let Ok(cmd) = config.read().editor() {
            let temp_file = temp_file("-repl-", ".md");
            let command = process::Command::new(cmd);
            editor = editor.with_buffer_editor(command, temp_file);
        }

        Ok(editor)
    }

    fn extra_keybindings(keybindings: &mut Keybindings) {
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu(MENU_NAME.to_string()),
                ReedlineEvent::MenuNext,
            ]),
        );
        keybindings.add_binding(
            KeyModifiers::SHIFT,
            KeyCode::BackTab,
            ReedlineEvent::MenuPrevious,
        );
        keybindings.add_binding(
            KeyModifiers::CONTROL,
            KeyCode::Enter,
            ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
        );
        keybindings.add_binding(
            KeyModifiers::CONTROL,
            KeyCode::Char('j'),
            ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
        );
    }

    fn create_edit_mode(config: &GlobalConfig) -> Box<dyn EditMode> {
        let edit_mode: Box<dyn EditMode> = if config.read().keybindings == "vi" {
            let mut insert_keybindings = default_vi_insert_keybindings();
            Self::extra_keybindings(&mut insert_keybindings);
            Box::new(Vi::new(insert_keybindings, default_vi_normal_keybindings()))
        } else {
            let mut keybindings = default_emacs_keybindings();
            Self::extra_keybindings(&mut keybindings);
            Box::new(Emacs::new(keybindings))
        };
        edit_mode
    }

    fn create_menu() -> ReedlineMenu {
        let completion_menu = ColumnarMenu::default().with_name(MENU_NAME);
        ReedlineMenu::EngineCompleter(Box::new(completion_menu))
    }
}

#[derive(Debug, Clone)]
pub struct ReplCommand {
    name: &'static str,
    description: &'static str,
    state: AssertState,
}

impl ReplCommand {
    fn new(name: &'static str, desc: &'static str, state: AssertState) -> Self {
        Self {
            name,
            description: desc,
            state,
        }
    }

    fn is_valid(&self, flags: StateFlags) -> bool {
        self.state.assert(flags)
    }
}

/// A default validator which checks for mismatched quotes and brackets
struct ReplValidator;

impl Validator for ReplValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        let line = line.trim();
        if line.starts_with(r#":::"#) && !line[3..].ends_with(r#":::"#) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

pub async fn run_repl_command(
    config: &GlobalConfig,
    abort_signal: AbortSignal,
    mut line: &str,
) -> Result<bool> {
    if let Ok(Some(captures)) = MULTILINE_RE.captures(line) {
        if let Some(text_match) = captures.get(1) {
            line = text_match.as_str();
        }
    }
    match parse_command(line) {
        Some((cmd, args)) => match cmd {
            ".help" => {
                dump_repl_help();
            }
            ".info" => match args {
                Some("role") => {
                    let info = config.read().role_info()?;
                    print!("{info}");
                }
                Some("session") => {
                    let info = config.read().session_info()?;
                    print!("{info}");
                }
                Some("rag") => {
                    let info = config.read().rag_info()?;
                    print!("{info}");
                }
                Some("agent") => {
                    let info = config.read().agent_info()?;
                    print!("{info}");
                }
                Some(_) => unknown_command()?,
                None => {
                    let output = config.read().sysinfo()?;
                    print!("{output}");
                }
            },
            ".model" => match args {
                Some(name) => {
                    config.write().set_model(name)?;
                }
                None => println!("用法：.model <name>"),
            },
            ".prompt" => match args {
                Some(text) => {
                    config.write().use_prompt(text)?;
                }
                None => println!("用法：.prompt <text>..."),
            },
            ".role" => match args {
                Some(args) => match args.split_once(['\n', ' ']) {
                    Some((name, text)) => {
                        let role = config.read().retrieve_role(name.trim())?;
                        let input = Input::from_str(config, text, Some(role));
                        ask(config, abort_signal.clone(), input, false).await?;
                    }
                    None => {
                        let name = args;
                        if !Config::has_role(name) {
                            config.write().new_role(name)?;
                        }
                        config.write().use_role(name)?;
                    }
                },
                None => println!(
                    r#"用法：
    .role <name>                    # 若角色已存在则切换到该角色，否则创建新角色
    .role <name> [text]...          # 临时切换到该角色，发送文本后再切回"#
                ),
            },
            ".session" => {
                config.write().use_session(args)?;
                Config::maybe_autoname_session(config.clone());
            }
            ".rag" => {
                Config::use_rag(config, args, abort_signal.clone()).await?;
            }
            ".agent" => match split_first_arg(args) {
                Some((agent_name, args)) => {
                    let (new_args, _) = split_args_text(args.unwrap_or_default(), cfg!(windows));
                    let (session_name, variable_pairs) = match new_args.first() {
                        Some(name) if name.contains('=') => (None, new_args.as_slice()),
                        Some(name) => (Some(name.as_str()), &new_args[1..]),
                        None => (None, &[] as &[String]),
                    };
                    let variables: AgentVariables = variable_pairs
                        .iter()
                        .filter_map(|v| v.split_once('='))
                        .map(|(key, value)| (key.to_string(), value.to_string()))
                        .collect();
                    if variables.len() != variable_pairs.len() {
                        bail!("部分变量未按 key=value 格式提供");
                    }
                    if !variables.is_empty() {
                        config.write().agent_variables = Some(variables);
                    }
                    let ret =
                        Config::use_agent(config, agent_name, session_name, abort_signal.clone())
                            .await;
                    config.write().agent_variables = None;
                    ret?;
                }
                None => {
                    println!(r#"用法：.agent <agent-name> [session-name] [key=value]..."#)
                }
            },
            ".starter" => match args {
                Some(id) => {
                    let mut text = None;
                    if let Some(agent) = config.read().agent.as_ref() {
                        for (i, value) in agent.conversation_staters().iter().enumerate() {
                            if (i + 1).to_string() == id {
                                text = Some(value.clone());
                            }
                        }
                    }
                    match text {
                        Some(text) => {
                            println!("{}", dimmed_text(&format!(">> {text}")));
                            let input = Input::from_str(config, &text, None);
                            ask(config, abort_signal.clone(), input, true).await?;
                        }
                        None => {
                            bail!("无效的对话开场白编号");
                        }
                    }
                }
                None => {
                    let banner = config.read().agent_banner()?;
                    config.read().print_markdown(&banner)?;
                }
            },
            ".save" => match split_first_arg(args) {
                Some(("role", name)) => {
                    config.write().save_role(name)?;
                }
                Some(("session", name)) => {
                    config.write().save_session(name)?;
                }
                _ => {
                    println!(r#"用法：.save <role|session> [name]"#)
                }
            },
            ".edit" => {
                if config.read().macro_flag {
                    bail!("宏执行期间无法执行此操作")
                }
                match args {
                    Some("config") => {
                        config.read().edit_config()?;
                    }
                    Some("role") => {
                        config.write().edit_role()?;
                    }
                    Some("session") => {
                        config.write().edit_session()?;
                    }
                    Some("rag-docs") => {
                        Config::edit_rag_docs(config, abort_signal.clone()).await?;
                    }
                    Some("agent-config") => {
                        config.write().edit_agent_config()?;
                    }
                    _ => {
                        println!(r#"用法：.edit <config|role|session|rag-docs|agent-config>"#)
                    }
                }
            }
            ".compress" => match args {
                Some("session") => {
                    abortable_run_with_spinner(
                        Config::compress_session(config),
                        "Compressing",
                        abort_signal.clone(),
                    )
                    .await?;
                    println!("✓ 会话已成功压缩。");
                }
                _ => {
                    println!(r#"用法：.compress session"#)
                }
            },
            ".empty" => match args {
                Some("session") => {
                    config.write().empty_session()?;
                }
                _ => {
                    println!(r#"用法：.empty session"#)
                }
            },
            ".rebuild" => match args {
                Some("rag") => {
                    Config::rebuild_rag(config, abort_signal.clone()).await?;
                }
                _ => {
                    println!(r#"用法：.rebuild rag"#)
                }
            },
            ".sources" => match args {
                Some("rag") => {
                    let output = Config::rag_sources(config)?;
                    println!("{output}");
                }
                _ => {
                    println!(r#"用法：.sources rag"#)
                }
            },
            ".macro" => match split_first_arg(args) {
                Some((name, extra)) => {
                    if !Config::has_macro(name) && extra.is_none() {
                        config.write().new_macro(name)?;
                    } else {
                        macro_execute(config, name, extra, abort_signal.clone()).await?;
                    }
                }
                None => println!("用法：.macro <name> <text>..."),
            },
            ".file" => match args {
                Some(args) => {
                    let (files, text) = split_args_text(args, cfg!(windows));
                    let input = Input::from_files_with_spinner(
                        config,
                        text,
                        files,
                        None,
                        abort_signal.clone(),
                    )
                    .await?;
                    ask(config, abort_signal.clone(), input, true).await?;
                }
                None => println!(
                    r#"用法：.file <file|dir|url|cmd|loader:resource|%%>... [-- <text>...]

.file /tmp/file.txt
.file src/ Cargo.toml -- analyze
.file https://example.com/file.txt -- summarize
.file https://example.com/image.png -- recognize text
.file `git diff` -- Generate git commit message
.file jina:https://example.com
.file %% -- 将上一条回复翻译成中文"#
                ),
            },
            ".continue" => {
                let LastMessage {
                    mut input, output, ..
                } = match config
                    .read()
                    .last_message
                    .as_ref()
                    .filter(|v| v.continuous && !v.output.is_empty())
                    .cloned()
                {
                    Some(v) => v,
                    None => bail!("没有可继续的上一条回答"),
                };
                input.set_continue_output(&output);
                ask(config, abort_signal.clone(), input, true).await?;
            }
            ".regenerate" => {
                let LastMessage { mut input, .. } = match config
                    .read()
                    .last_message
                    .as_ref()
                    .filter(|v| v.continuous)
                    .cloned()
                {
                    Some(v) => v,
                    None => bail!("没有可重新生成的上一条回答"),
                };
                input.set_regenerate();
                ask(config, abort_signal.clone(), input, true).await?;
            }
            ".set" => match args {
                Some(args) => {
                    Config::update(config, args)?;
                }
                _ => {
                    println!("用法：.set <key> <value>...")
                }
            },
            ".delete" => match args {
                Some(args) => {
                    Config::delete(config, args)?;
                }
                _ => {
                    println!("用法：.delete <role|session|rag|macro|agent-data>")
                }
            },
            ".copy" => {
                let output = match config
                    .read()
                    .last_message
                    .as_ref()
                    .filter(|v| !v.output.is_empty())
                    .map(|v| v.output.clone())
                {
                    Some(v) => v,
                    None => bail!("没有可复制的聊天回复"),
                };
                set_text(&output).context("Failed to copy the last chat response")?;
            }
            ".exit" => match args {
                Some("role") => {
                    config.write().exit_role()?;
                }
                Some("session") => {
                    if config.read().agent.is_some() {
                        config.write().exit_agent_session()?;
                    } else {
                        config.write().exit_session()?;
                    }
                }
                Some("rag") => {
                    config.write().exit_rag()?;
                }
                Some("agent") => {
                    config.write().exit_agent()?;
                }
                Some(_) => unknown_command()?,
                None => {
                    return Ok(true);
                }
            },
            ".clear" => match args {
                Some("messages") => {
                    bail!("请改用 '.empty session'");
                }
                _ => unknown_command()?,
            },
            _ => unknown_command()?,
        },
        None => {
            let input = Input::from_str(config, line, None);
            ask(config, abort_signal.clone(), input, true).await?;
        }
    }

    if !config.read().macro_flag {
        println!();
    }

    Ok(false)
}

#[async_recursion::async_recursion]
async fn ask(
    config: &GlobalConfig,
    abort_signal: AbortSignal,
    mut input: Input,
    with_embeddings: bool,
) -> Result<()> {
    if input.is_empty() {
        return Ok(());
    }
    if with_embeddings {
        input.use_embeddings(abort_signal.clone()).await?;
    }
    while config.read().is_compressing_session() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let client = input.create_client()?;
    config.write().before_chat_completion(&input)?;
    let (output, tool_results) = if input.stream() {
        call_chat_completions_streaming(&input, client.as_ref(), abort_signal.clone()).await?
    } else {
        call_chat_completions(&input, true, false, client.as_ref(), abort_signal.clone()).await?
    };
    config
        .write()
        .after_chat_completion(&input, &output, &tool_results)?;
    if !tool_results.is_empty() {
        ask(
            config,
            abort_signal,
            input.merge_tool_results(output, tool_results),
            false,
        )
        .await
    } else {
        Config::maybe_autoname_session(config.clone());
        Config::maybe_compress_session(config.clone());
        Ok(())
    }
}

fn unknown_command() -> Result<()> {
    bail!(r#"Unknown command. 输入 ".help" 查看帮助。"#);
}

fn dump_repl_help() {
    let head = REPL_COMMANDS
        .iter()
        .map(|cmd| format!("{:<24} {}", cmd.name, cmd.description))
        .collect::<Vec<String>>()
        .join("\n");
    println!(
        r###"{head}

输入 ::: 开始多行编辑，再次输入 ::: 结束编辑。
按 Ctrl+O 可调用外部编辑器编辑输入内容。
按 Ctrl+C 取消当前回答，按 Ctrl+D 退出交互模式。"###,
    );
}

fn parse_command(line: &str) -> Option<(&str, Option<&str>)> {
    match COMMAND_RE.captures(line) {
        Ok(Some(captures)) => {
            let cmd = captures.get(1)?.as_str();
            let args = line[captures[0].len()..].trim();
            let args = if args.is_empty() { None } else { Some(args) };
            Some((cmd, args))
        }
        _ => None,
    }
}

fn split_first_arg(args: Option<&str>) -> Option<(&str, Option<&str>)> {
    args.map(|v| match v.split_once(' ') {
        Some((subcmd, args)) => (subcmd, Some(args.trim())),
        None => (v, None),
    })
}

pub fn split_args_text(line: &str, is_win: bool) -> (Vec<String>, &str) {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut unbalance: Option<char> = None;
    let mut prev_char: Option<char> = None;
    let mut text_starts_at = None;
    let unquote_word = |word: &str| {
        if ((word.starts_with('"') && word.ends_with('"'))
            || (word.starts_with('\'') && word.ends_with('\'')))
            && word.len() >= 2
        {
            word[1..word.len() - 1].to_string()
        } else {
            word.to_string()
        }
    };
    let chars: Vec<char> = line.chars().collect();

    for (i, char) in chars.iter().cloned().enumerate() {
        match unbalance {
            Some(ub_char) if ub_char == char => {
                word.push(char);
                unbalance = None;
            }
            Some(_) => {
                word.push(char);
            }
            None => match char {
                ' ' | '\t' | '\r' | '\n' => {
                    if char == '\r' && chars.get(i + 1) == Some(&'\n') {
                        continue;
                    }
                    if let Some('\\') = prev_char.filter(|_| !is_win) {
                        word.push(char);
                    } else if !word.is_empty() {
                        if word == "--" {
                            word.clear();
                            text_starts_at = Some(i + 1);
                            break;
                        }
                        words.push(unquote_word(&word));
                        word.clear();
                    }
                }
                '\'' | '"' | '`' => {
                    word.push(char);
                    unbalance = Some(char);
                }
                '\\' => {
                    if is_win || prev_char.map(|c| c == '\\').unwrap_or_default() {
                        word.push(char);
                    }
                }
                _ => {
                    word.push(char);
                }
            },
        }
        prev_char = Some(char);
    }

    if !word.is_empty() && word != "--" {
        words.push(unquote_word(&word));
    }
    let text = match text_starts_at {
        Some(start) => &line[start..],
        None => "",
    };

    (words, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_command_line() {
        assert_eq!(parse_command(" ."), Some((".", None)));
        assert_eq!(parse_command(" .role"), Some((".role", None)));
        assert_eq!(parse_command(" .role  "), Some((".role", None)));
        assert_eq!(
            parse_command(" .set dry_run true"),
            Some((".set", Some("dry_run true")))
        );
        assert_eq!(
            parse_command(" .set dry_run true  "),
            Some((".set", Some("dry_run true")))
        );
        assert_eq!(
            parse_command(".prompt \nabc\n"),
            Some((".prompt", Some("abc")))
        );
    }

    #[test]
    fn test_split_args_text() {
        assert_eq!(split_args_text("", false), (vec![], ""));
        assert_eq!(
            split_args_text("file.txt", false),
            (vec!["file.txt".into()], "")
        );
        assert_eq!(
            split_args_text("file.txt --", false),
            (vec!["file.txt".into()], "")
        );
        assert_eq!(
            split_args_text("file.txt -- hello", false),
            (vec!["file.txt".into()], "hello")
        );
        assert_eq!(
            split_args_text("file.txt -- \thello", false),
            (vec!["file.txt".into()], "\thello")
        );
        assert_eq!(
            split_args_text("file.txt --\nhello", false),
            (vec!["file.txt".into()], "hello")
        );
        assert_eq!(
            split_args_text("file.txt --\r\nhello", false),
            (vec!["file.txt".into()], "hello")
        );
        assert_eq!(
            split_args_text("file.txt --\rhello", false),
            (vec!["file.txt".into()], "hello")
        );
        assert_eq!(
            split_args_text(r#"file1.txt 'file2.txt' "file3.txt""#, false),
            (
                vec!["file1.txt".into(), "file2.txt".into(), "file3.txt".into()],
                ""
            )
        );
        assert_eq!(
            split_args_text(r#"./file1.txt 'file1 - Copy.txt' file\ 2.txt"#, false),
            (
                vec![
                    "./file1.txt".into(),
                    "file1 - Copy.txt".into(),
                    "file 2.txt".into()
                ],
                ""
            )
        );
        assert_eq!(
            split_args_text(r#".\file.txt C:\dir\file.txt"#, true),
            (vec![".\\file.txt".into(), "C:\\dir\\file.txt".into()], "")
        );
    }
}
