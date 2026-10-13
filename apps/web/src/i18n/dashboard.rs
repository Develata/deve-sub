//! Existing dashboard states and chart labels.
use super::Language;

pub(super) fn t(lang: Language, key: &str) -> &str {
    match (lang, key) {
        (Language::Zh, "dashboard.summary") => "流量汇总",
        (Language::En, "dashboard.summary") => "Traffic summary",
        (Language::Zh, "dashboard.total_upload") => "总上传",
        (Language::En, "dashboard.total_upload") => "Total upload",
        (Language::Zh, "dashboard.total_download") => "总下载",
        (Language::En, "dashboard.total_download") => "Total download",
        (Language::Zh, "dashboard.total") => "总计",
        (Language::En, "dashboard.total") => "Total traffic",
        (Language::Zh, "dashboard.history") => "30 天流量趋势",
        (Language::En, "dashboard.history") => "30-day traffic history",
        (Language::Zh, "dashboard.daily_utc") => "按日统计 · UTC 日期",
        (Language::En, "dashboard.daily_utc") => "Daily totals · UTC dates",
        (Language::Zh, "dashboard.loading_summary") => "正在加载流量汇总…",
        (Language::En, "dashboard.loading_summary") => "Loading traffic summary…",
        (Language::Zh, "dashboard.loading_history") => "正在加载流量历史…",
        (Language::En, "dashboard.loading_history") => "Loading traffic history…",
        (Language::Zh, "dashboard.summary_failed") => "流量汇总加载失败",
        (Language::En, "dashboard.summary_failed") => "Could not load traffic summary",
        (Language::Zh, "dashboard.history_failed") => "流量历史加载失败",
        (Language::En, "dashboard.history_failed") => "Could not load traffic history",
        (Language::Zh, "dashboard.empty_history") => "这段时间没有流量历史记录",
        (Language::En, "dashboard.empty_history") => "No traffic history for this period",
        (Language::Zh, "dashboard.retry") => "重试",
        (Language::En, "dashboard.retry") => "Retry",
        (Language::Zh, "dashboard.upload") => "上传",
        (Language::En, "dashboard.upload") => "Upload",
        (Language::Zh, "dashboard.download") => "下载",
        (Language::En, "dashboard.download") => "Download",
        (Language::Zh, "dashboard.date") => "日期（UTC）",
        (Language::En, "dashboard.date") => "Date (UTC)",
        (Language::Zh, "dashboard.scroll_hint") => "左右滑动查看日期和流量刻度",
        (Language::En, "dashboard.scroll_hint") => {
            "Scroll horizontally to view dates and traffic values"
        }
        _ => key,
    }
}
