//! 从宿主额度窗口中找出 Codex 主额度并按阈值判定。
//! primary/secondary 不固定对应 5 小时或周，按窗口时长区分；Spark 等附加额度的限流桶 ID 不是 `codex`，不参与判定。

use super::{records::Trigger, settings::AutoSettings};
use crate::host_admin::QuotaWindow;
use gateway_plugin_sdk::call::data::QuotaFacts;
use std::ops::RangeInclusive;

const CODEX_LIMIT_ID: &str = "codex";
const SHORT_TERM_SECONDS: RangeInclusive<u64> = 1..=6 * 3600;
const WEEKLY_SECONDS: RangeInclusive<u64> = 6 * 86_400..=8 * 86_400;

#[derive(Default)]
pub(super) struct MainWindows<'a> {
    weekly: Option<&'a QuotaWindow>,
    short_term: Option<&'a QuotaWindow>,
}

pub(super) struct Fresh {
    pub trigger: Option<Trigger>,
    pub weekly_reset_at_ms: Option<i64>,
}

pub(super) fn main_windows(windows: &[QuotaWindow]) -> MainWindows<'_> {
    let mut main = MainWindows::default();
    for window in windows {
        if !window
            .limit_id
            .as_deref()
            .is_some_and(|limit| limit.eq_ignore_ascii_case(CODEX_LIMIT_ID))
        {
            continue;
        }
        match window.window_seconds {
            Some(seconds) if WEEKLY_SECONDS.contains(&seconds) => main.weekly = Some(window),
            Some(seconds) if SHORT_TERM_SECONDS.contains(&seconds) => {
                main.short_term = Some(window);
            }
            _ => {}
        }
    }
    main
}

/// 用宿主已有观测粗筛；触顶标记也视为达到阈值。
pub(super) fn cached_trigger(
    windows: &MainWindows<'_>,
    settings: &AutoSettings,
) -> Option<Trigger> {
    let reached = |window: Option<&QuotaWindow>, threshold: Option<u8>| {
        window.zip(threshold).is_some_and(|(window, threshold)| {
            window.limit_reached || exceeds(window.used_percent, threshold)
        })
    };
    combine(
        reached(windows.weekly, Some(settings.weekly_threshold)),
        reached(windows.short_term, settings.short_term_threshold),
    )
}

/// 用刚刷新的上游观测复核，并取周额度重置时间作为周期标识。
pub(super) fn fresh_state(
    facts: &QuotaFacts,
    windows: &MainWindows<'_>,
    settings: &AutoSettings,
) -> Fresh {
    let find = |window: Option<&QuotaWindow>| {
        window.and_then(|window| facts.windows.iter().find(|fact| fact.key == window.key))
    };
    let weekly = find(windows.weekly);
    let short_term = find(windows.short_term);
    Fresh {
        trigger: combine(
            weekly.is_some_and(|fact| exceeds(fact.used_percent, settings.weekly_threshold)),
            short_term
                .zip(settings.short_term_threshold)
                .is_some_and(|(fact, threshold)| exceeds(fact.used_percent, threshold)),
        ),
        weekly_reset_at_ms: weekly.and_then(|fact| fact.reset_at_ms),
    }
}

fn exceeds(used_percent: Option<f64>, threshold: u8) -> bool {
    used_percent.is_some_and(|used| used >= f64::from(threshold))
}

fn combine(weekly: bool, short_term: bool) -> Option<Trigger> {
    match (weekly, short_term) {
        (true, true) => Some(Trigger::Both),
        (true, false) => Some(Trigger::Weekly),
        (false, true) => Some(Trigger::ShortTerm),
        (false, false) => None,
    }
}
