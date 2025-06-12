use owo_colors::OwoColorize;

pub mod config;
pub mod constants;
pub mod crypto;
pub mod error;
pub mod format;
pub mod logging;

#[macro_export]
macro_rules! success {
    ($($arg:tt)*) => {
        {
            use owo_colors::OwoColorize;
            println!("{} {}", "✓".green(), format!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! warning {
    ($($arg:tt)*) => {
        {
            use owo_colors::OwoColorize;
            println!("{} {}", "⚠".yellow(), format!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
       {
            use owo_colors::OwoColorize;
            println!("{} {}", "ℹ".blue(), format!($($arg)*))
       }
    };
}

pub fn reduced_node_id(node_id: &iroh::NodeId) -> String {
    let id_str = node_id.to_string();
    format!(
        "{}{}{}",
        (&id_str[..6]).bold().blue(),
        "...".dimmed(),
        (&id_str[id_str.len() - 6..]).bold().blue()
    )
}
