pub mod quant;

#[derive(Debug, Clone)]
pub struct Decision {
    pub goto: String,
    pub label: String,
    pub confidence: f64,
    pub rationale: String,
}
