use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenConfigDto { pub path: String, pub name: Option<String>, pub frozen: bool, pub error: Option<String> }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenReasonDto { pub tree: String, pub leaf: String, pub score: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenPickDto {
    pub rank: usize, pub symbol: String,
    pub quality_score: f64, pub speculative_score: f64, pub combined_score: f64,
    pub tags: Vec<String>, pub selected: bool, pub reasons: Vec<ScreenReasonDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenResultDto {
    pub config: String, pub as_of: String,
    pub n_universe: usize, pub top: usize, pub rows: Vec<ScreenPickDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScreenRunMetaDto {
    pub id: String, pub config: String, pub from: String, pub to: String,
    pub top: u32, pub rebalance: u32, pub created: String, pub ok: bool, pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NavPointDto { pub t: String, pub nav: f64, pub benchmark_nav: f64 }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TagAttribDto { pub tag: String, pub n_picks: usize, pub hit_rate: f64, pub mean_fwd_return: f64 }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RegimeSliceDto { pub label: String, pub from: String, pub to: String, pub picks_return: f64, pub benchmark_return: f64, pub excess: f64 }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct QualityLayerDto { pub layer: usize, pub n: usize, pub mean_quality: f64, pub mean_fwd_return: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenBacktestReportDto {
    pub meta: ScreenRunMetaDto,
    pub net_total_return: f64, pub gross_total_return: f64,
    pub abs_sharpe: Option<f64>, pub max_drawdown: f64, pub turnover: f64,
    pub break_even: Option<f64>,
    pub nav: Vec<NavPointDto>,
    pub tag_attribution: Vec<TagAttribDto>,
    pub regime_slices: Vec<RegimeSliceDto>,
    pub quality_layers: Vec<QualityLayerDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ExcessPointDto { pub t: String, pub excess: f64 }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RegimeExcessDto { pub label: String, pub excess: Option<f64> }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct IndexRelativeDto {
    pub benchmark: String, pub excess_cum: Option<f64>,
    pub curve: Vec<ExcessPointDto>, pub per_regime: Vec<RegimeExcessDto>,
}
