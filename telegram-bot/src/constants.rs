pub(crate) mod streaming {
    pub const EDIT_THROTTLE_BASE_MS: u64 = 300;

    pub const SMALL_MESSAGE_THRESHOLD: usize = 200;

    pub const LARGE_MESSAGE_THRESHOLD: usize = 3000;
}

pub(crate) mod retry {
    pub const MAX_RETRIES: u32 = 3;
}

pub(crate) mod model {
    pub const SEARCH_PAGE_SIZE: usize = 8;
}

pub(crate) mod download {
    pub const MAX_FILE_ID_LEN: usize = 24;

    pub const MAX_EXT_LEN: usize = 10;
}
