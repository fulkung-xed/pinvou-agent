//! 一次性 dump: 复现 pinvou3 新建 session 时,底座 `Engine::new` 在
//! `core/engine.rs:1227` 拼出来的真实 system prompt。
//!
//! 不 spawn engine(避免连 vLLM/起 turn loop),只复刻 prompt 拼装环节:
//!   1. bridge.boot()                                  -> ensure_dirs + load prefs
//!      (boot 内 install_prompt_overrides() 安装 pinvou3 composer/base override)
//!   2. bridge.build_engine_config_for_session(sid)    -> EngineConfig(含 inline instructions)
//!   3. prompts::system_prompt_for_mode_with_context_skills_session_and_approval(...)
//!
//! 跑法:
//!   cargo run --bin dump_system_prompt \
//!     --manifest-path pinvou3-app/src-tauri/Cargo.toml \
//!     > /tmp/pinvou3_system_prompt.txt

use anyhow::Result;
use deepseek_tui::prompts::{self, PromptSessionContext};
use deepseek_tui::tui::app::AppMode;
use deepseek_tui::tui::approval::ApprovalMode;
use pinvou3_lib::features::assistant::platform::bridge::Pinvou3Bridge;

fn main() -> Result<()> {
    // session id 走临时值,避免污染真实 sessions/
    let sid = "__dump_system_prompt__";

    let bridge = Pinvou3Bridge::boot()?;
    let cfg = bridge.build_engine_config_for_session(sid);

    // 复刻 Engine::new (core/engine.rs:1210-1254) 的入参装配。
    //
    // user_memory_block: 上游 v0.9.x 把 `memory` 模块换成的 `native_memory` 是
    // crate-private,外部 bin 调不到 `native_prompt_block`。pinvou3 bridge 恒置
    // `memory_enabled: false`(bridge.rs:1285),`native_prompt_block(false, ..)`
    // 第一行即返回 None,因此这里直接取 None 与生产逐字节等价。
    let user_memory_block: Option<String> = None;

    let session_ctx = PromptSessionContext {
        user_memory_block: user_memory_block.as_deref(),
        goal_objective: None, // Engine::new 里通过 goal_objective_for_prompt 算,新 session 无 goal => None
        project_context_pack_enabled: cfg.project_context_pack_enabled,
        locale_tag: &cfg.locale_tag,
        translation_enabled: cfg.translation_enabled,
        model_id: &cfg.model,
        // v0.8.57:上游把 allow_shell 从 PromptSessionContext 移除(#2949,decouple
        // 静态前缀,allow_shell 改走 per-turn <runtime_prompt> tag),dump 同步去掉。
        // v0.9.6:`show_thinking` 字段已从 PromptSessionContext/EngineConfig 删除
        // (思考开关改走 reasoning_effort),同步去掉。
        // v0.8.60 上游新增字段:
        //   context_window_override: None = 用 model_id 派生的默认窗口。生产 Engine::new
        //   传 Some(route_context_window_tokens(..)),但 v0.9.6 起 prompt 合成不再打印
        //   context-window 事实(见 PromptSessionContext 字段注释),且 pinvou3 composer
        //   忽略 ctx 恒返回 MODE_EXECUTE_MD → None 与生产逐字节等价。
        context_window_override: None,
        // verbosity: None = GUI 不用 concise 输出模式(与 bridge Op::SendMessage 一致)。
        verbosity: None,
        // CodeWhale r4 后技能发现只使用显式 EngineConfig.skills_dir；当前 app 注入
        // ~/.pinvou3/bundle/skills(技能市场私有区,两 mode 行为一致)。底座不再追加
        // 其他根，因此此兼容扫描开关无实际影响，取 false。
        skills_scan_codewhale_only: false,
        // v0.9.6 上游新增字段:
        //   plugin_registry: PluginRegistry 类型 crate-private,外部只能传 None;
        //   生产侧 bridge 置 EngineConfig.plugin_registry=None → Engine::new 自建
        //   PluginRegistry::empty(workspace),空注册表不贡献任何 plugin skill,
        //   与 None 渲染结果等价。
        plugin_registry: None,
        //   mode: Engine::new 恒传 AppMode::Agent(注释:与 current_mode 初值一致,
        //   /mode 切换走 refresh_system_prompt)。bundled prompt 文案刻意忽略它,
        //   pinvou3 composer 也不读 mode,仅保持语义对齐。
        mode: AppMode::Agent,
    };

    // 生产路径 Engine::new 调的是 pub(crate) 的 `_for_host` 变体,prompt_host 由
    // `terminal_chrome_enabled` 决定;bridge 透传上游 default(true)→ Interactive。
    // 公开的 `system_prompt_for_mode_with_context_skills_session_and_approval` 正是
    // `PromptHost::Interactive` 的委托包装(prompts.rs:1055-1070),与生产逐字节等价。
    //
    // pinvou3 composer(apply_static_prompt_composer)不读 mode → dump 出的静态层
    // 恒为 Execute 块,与下方 plan/agent 参数无关(仅用于 meta 展示)。
    let (mode, approval) = match std::env::args().nth(1).as_deref() {
        Some("plan") => (AppMode::Plan, ApprovalMode::Never),
        Some("agent") => (AppMode::Agent, ApprovalMode::Suggest),
        _ => (AppMode::Yolo, ApprovalMode::Auto),
    };
    let prompt = prompts::system_prompt_for_mode_with_context_skills_session_and_approval(
        &cfg.workspace,
        None,
        Some(&cfg.skills_dir),
        Some(&cfg.instructions),
        session_ctx,
    );
    let text = prompts::system_prompt_flat_text(&prompt);

    // 打 dump 元数据 + body。v0.9.0 生产路径返回 Blocks，统一扁平化便于跨版本 diff。
    eprintln!("───────── dump meta ─────────");
    eprintln!("workspace    = {}", cfg.workspace.display());
    eprintln!("skills_dir   = {}", cfg.skills_dir.display());
    eprintln!("instructions = {:?}", cfg.instructions);
    eprintln!("locale_tag   = {}", cfg.locale_tag);
    eprintln!("model_id     = {}", cfg.model);
    eprintln!("approval     = {approval:?}");
    eprintln!("mode         = {mode:?}");
    eprintln!("byte_len     = {}", text.len());
    eprintln!("line_count   = {}", text.lines().count());
    eprintln!("─────────────────────────────");
    println!("{text}");
    Ok(())
}
