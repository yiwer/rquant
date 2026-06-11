//! 回测框架：成本模型、前瞻收益计算、数据缺口检测、度量聚合、硬/软运行器及 walk-forward。

pub mod costs;
pub mod forward_return;
pub mod gaps;
pub mod metrics;
pub mod portfolio;
pub mod runner;
pub mod sim;
pub mod soft;
pub mod walkforward;
