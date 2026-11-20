/// User-facing language selection for the Desktop application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    English,
    Vietnamese,
}

impl Locale {
    /// Parses a locale code such as `en` or `vi`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "en" | "en-us" => Some(Self::English),
            "vi" | "vi-vn" => Some(Self::Vietnamese),
            _ => None,
        }
    }

    /// Reads the locale from the environment, defaulting to English.
    #[must_use]
    pub fn from_env() -> Self {
        std::env::var("TM_TRADING_LOCALE")
            .ok()
            .and_then(|value| Self::parse(&value))
            .unwrap_or_default()
    }
}

/// Every user-facing string in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiStrings {
    pub app_title: &'static str,
    pub control_center: &'static str,
    pub services_and_agents: &'static str,
    pub service_column: &'static str,
    pub agent_column: &'static str,
    pub desired_column: &'static str,
    pub observed_column: &'static str,
    pub readiness_column: &'static str,
    pub restarts_column: &'static str,
    pub last_exit_column: &'static str,
    pub actions_column: &'static str,
    pub start: &'static str,
    pub stop: &'static str,
    pub restart: &'static str,
    pub connected: &'static str,
    pub connecting: &'static str,
    pub disconnected: &'static str,
    pub loading: &'static str,
    pub no_services: &'static str,
    pub no_services_hint: &'static str,
    pub operation: &'static str,
    pub operation_pending: &'static str,
    pub operation_none: &'static str,
    pub refreshed_at: &'static str,
    pub language: &'static str,
    pub worker_instance: &'static str,
    pub watchlists: &'static str,
    pub watchlist_new_placeholder: &'static str,
    pub watchlist_add: &'static str,
    pub watchlist_default: &'static str,
    pub watchlist_symbols_placeholder: &'static str,
    pub watchlist_save: &'static str,
    pub watchlist_delete: &'static str,
    pub watchlist_empty: &'static str,
    pub watchlist_empty_hint: &'static str,
    pub watchlist_symbols_count: &'static str,
    pub market_overview: &'static str,
    pub market_pulse: &'static str,
    pub market_empty: &'static str,
    pub market_empty_hint: &'static str,
    pub market_symbol_column: &'static str,
    pub market_last_close_column: &'static str,
    pub market_change_column: &'static str,
    pub market_volume_column: &'static str,
    pub market_volume_ratio_column: &'static str,
    pub market_volatility_column: &'static str,
    pub market_age_column: &'static str,
    pub market_status_column: &'static str,
    pub market_state_ok: &'static str,
    pub market_state_partial: &'static str,
    pub market_state_stale: &'static str,
    pub market_state_empty: &'static str,
}

impl UiStrings {
    #[must_use]
    pub fn for_locale(locale: Locale) -> Self {
        match locale {
            Locale::English => Self::english(),
            Locale::Vietnamese => Self::vietnamese(),
        }
    }

    fn english() -> Self {
        Self {
            app_title: "TM Trading Control Center",
            control_center: "Control Center",
            services_and_agents: "Services & Agents",
            service_column: "Service",
            agent_column: "Agent",
            desired_column: "Desired",
            observed_column: "Observed",
            readiness_column: "Readiness",
            restarts_column: "Restarts",
            last_exit_column: "Last exit",
            actions_column: "Actions",
            start: "Start",
            stop: "Stop",
            restart: "Restart",
            connected: "Connected",
            connecting: "Connecting",
            disconnected: "Disconnected",
            loading: "Loading services…",
            no_services: "No services reported",
            no_services_hint: "The Agent has not reported a service yet.",
            operation: "Operation",
            operation_pending: "Waiting for the Agent to finish…",
            operation_none: "No operation has been requested yet.",
            refreshed_at: "Refreshed",
            language: "Language",
            worker_instance: "Instance",
            watchlists: "Watchlists",
            watchlist_new_placeholder: "New list name",
            watchlist_add: "Add list",
            watchlist_default: "Default",
            watchlist_symbols_placeholder: "BTCUSDT, ETHUSDT",
            watchlist_save: "Save symbols",
            watchlist_delete: "Delete",
            watchlist_empty: "No watchlists yet",
            watchlist_empty_hint: "Create a list to keep the instruments you follow.",
            watchlist_symbols_count: "symbols",
            market_overview: "Market Overview",
            market_pulse: "Market Pulse",
            market_empty: "No closed candles yet",
            market_empty_hint: "The Market Worker has not recorded closed candles for these symbols.",
            market_symbol_column: "Symbol",
            market_last_close_column: "Last close",
            market_change_column: "Change %",
            market_volume_column: "Volume",
            market_volume_ratio_column: "Volume ratio",
            market_volatility_column: "Volatility %",
            market_age_column: "Data age",
            market_status_column: "State",
            market_state_ok: "OK",
            market_state_partial: "Partial",
            market_state_stale: "Stale",
            market_state_empty: "Empty",
        }
    }

    fn vietnamese() -> Self {
        Self {
            app_title: "TM Trading Control Center",
            control_center: "Trung tâm điều khiển",
            services_and_agents: "Dịch vụ & Agent",
            service_column: "Dịch vụ",
            agent_column: "Agent",
            desired_column: "Mong muốn",
            observed_column: "Quan sát",
            readiness_column: "Sẵn sàng",
            restarts_column: "Khởi động lại",
            last_exit_column: "Mã thoát",
            actions_column: "Thao tác",
            start: "Khởi động",
            stop: "Dừng",
            restart: "Khởi động lại",
            connected: "Đã kết nối",
            connecting: "Đang kết nối",
            disconnected: "Mất kết nối",
            loading: "Đang tải dịch vụ…",
            no_services: "Chưa có dịch vụ nào",
            no_services_hint: "Agent chưa báo cáo dịch vụ nào.",
            operation: "Thao tác",
            operation_pending: "Đang chờ Agent hoàn tất…",
            operation_none: "Chưa có thao tác nào được yêu cầu.",
            refreshed_at: "Cập nhật lúc",
            language: "Ngôn ngữ",
            worker_instance: "Phiên bản",
            watchlists: "Danh sách theo dõi",
            watchlist_new_placeholder: "Tên danh sách mới",
            watchlist_add: "Thêm danh sách",
            watchlist_default: "Mặc định",
            watchlist_symbols_placeholder: "BTCUSDT, ETHUSDT",
            watchlist_save: "Lưu mã",
            watchlist_delete: "Xoá",
            watchlist_empty: "Chưa có danh sách nào",
            watchlist_empty_hint: "Tạo danh sách để lưu các mã bạn theo dõi.",
            watchlist_symbols_count: "mã",
            market_overview: "Tổng quan thị trường",
            market_pulse: "Nhịp thị trường",
            market_empty: "Chưa có nến đóng cửa",
            market_empty_hint: "Market Worker chưa ghi nến đóng cửa cho các mã này.",
            market_symbol_column: "Mã",
            market_last_close_column: "Giá đóng",
            market_change_column: "Thay đổi %",
            market_volume_column: "Khối lượng",
            market_volume_ratio_column: "Tỷ lệ khối lượng",
            market_volatility_column: "Biến động %",
            market_age_column: "Tuổi dữ liệu",
            market_status_column: "Trạng thái",
            market_state_ok: "OK",
            market_state_partial: "Một phần",
            market_state_stale: "Cũ",
            market_state_empty: "Trống",
        }
    }
}
