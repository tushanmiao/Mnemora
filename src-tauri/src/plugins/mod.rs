mod manager;
pub mod types;

pub(crate) use manager::prepare_plugin_package;
pub use manager::PluginManager;
pub use types::{PluginInstallRequest, PluginOverview, PluginSummary};
