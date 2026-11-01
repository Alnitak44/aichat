use anyhow::{Context, Result};
use clap::Parser;
use is_terminal::IsTerminal;
use std::io::{stdin, Read};

#[derive(Parser, Debug)]
#[command(author, version, about = "多模型 AI 对话命令行工具", long_about = None)]
pub struct Cli {
    /// 选择要使用的语言模型
    #[clap(short, long)]
    pub model: Option<String>,
    /// 使用自定义系统提示词
    #[clap(long)]
    pub prompt: Option<String>,
    /// 选择角色
    #[clap(short, long)]
    pub role: Option<String>,
    /// 创建或加入会话
    #[clap(short = 's', long)]
    pub session: Option<Option<String>>,
    /// 确保会话为空
    #[clap(long)]
    pub empty_session: bool,
    /// 将本次新对话保存到会话
    #[clap(long)]
    pub save_session: bool,
    /// 启动智能体
    #[clap(short = 'a', long)]
    pub agent: Option<String>,
    /// 设置智能体变量
    #[clap(long, value_names = ["NAME", "VALUE"], num_args = 2)]
    pub agent_variable: Vec<String>,
    /// 启动 RAG 知识库
    #[clap(long)]
    pub rag: Option<String>,
    /// 重新构建 RAG 以同步文档变更
    #[clap(long)]
    pub rebuild_rag: bool,
    /// 执行宏
    #[clap(long = "macro", value_name = "MACRO")]
    pub macro_name: Option<String>,
    /// 启动 LLM API 服务和 Web 应用
    #[clap(long, value_name = "ADDRESS")]
    pub serve: Option<Option<String>>,
    /// 使用自然语言执行命令
    #[clap(short = 'e', long)]
    pub execute: bool,
    /// 仅输出代码
    #[clap(short = 'c', long)]
    pub code: bool,
    /// 附加文件、目录或 URL
    #[clap(short = 'f', long, value_name = "FILE")]
    pub file: Vec<String>,
    /// 关闭流式输出
    #[clap(short = 'S', long)]
    pub no_stream: bool,
    /// 仅显示消息，不发送给模型
    #[clap(long)]
    pub dry_run: bool,
    /// 显示运行信息
    #[clap(long)]
    pub info: bool,
    /// 同步模型列表更新
    #[clap(long)]
    pub sync_models: bool,
    /// 列出所有可用的对话模型
    #[clap(long)]
    pub list_models: bool,
    /// 列出所有角色
    #[clap(long)]
    pub list_roles: bool,
    /// 列出所有会话
    #[clap(long)]
    pub list_sessions: bool,
    /// 列出所有智能体
    #[clap(long)]
    pub list_agents: bool,
    /// 列出所有 RAG 知识库
    #[clap(long)]
    pub list_rags: bool,
    /// 列出所有宏
    #[clap(long)]
    pub list_macros: bool,
    /// 输入文本
    #[clap(trailing_var_arg = true)]
    text: Vec<String>,
}

impl Cli {
    pub fn text(&self) -> Result<Option<String>> {
        let mut stdin_text = String::new();
        if !stdin().is_terminal() {
            let _ = stdin()
                .read_to_string(&mut stdin_text)
                .context("标准输入管道无效")?;
        };
        match self.text.is_empty() {
            true => {
                if stdin_text.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(stdin_text))
                }
            }
            false => {
                if self.macro_name.is_some() {
                    let text = self
                        .text
                        .iter()
                        .map(|v| shell_words::quote(v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if stdin_text.is_empty() {
                        Ok(Some(text))
                    } else {
                        Ok(Some(format!("{text} -- {stdin_text}")))
                    }
                } else {
                    let text = self.text.join(" ");
                    if stdin_text.is_empty() {
                        Ok(Some(text))
                    } else {
                        Ok(Some(format!("{text}\n{stdin_text}")))
                    }
                }
            }
        }
    }
}
