# Ridge 消融研究 设计(train-ablation 分支)

> 日期 2026-06-24 · 分支 train-ablation(off master 6b6320f)· 标准 §5.3
> 用户已批准:**目标 = 消融/理解性研究**(刻画各轴效应,不求破 ridge 上限、不动部署);
> idea4 分类用**无监督聚类(KMeans/GMM on gauss 因子)**。

## 目标与非目标
- **目标**:在已验证的 ridge-on-gauss 基线上,系统刻画 4 个轴的**效应**,诚实判读(多半会重验"墙",
  尤其量化 idea4 分模型的过拟合代价)。成功 = 干净可信的刻画,**不是**新 deploy 候选。
- **非目标**:不改引擎、不动冻结部署、不动 72 因子集、不求超过 ridge(若某轴真超且过 §5.3 才另议)。

## 架构(方案 A)
单一消融 harness `scripts/train_ablation.py`,**复用 `eval_ridge` 闭式原语**(`fit_ridge`/`backtest_ridge`/
`backtest_rank_linear`/`select_delta_ridge`/`_eligible`/`norm_gauss`)+ `iterate.to_index_relative`。
4 个轴各自围绕 ridge 基线独立扫,**同一套 §5.3 六折 eval**。不复用 `train_iterative.py`(SGD+PCA,
findings 已证 PCA 更差/SGD 不胜闭式,会混淆轴)。

## 统一评估口径
- 折:`eval_ridge_windows.FOLDS`(2020–2025/26 共 6 折,membership 池)。
- 每变体逐折:TRAIN 拟合 → OOS 周频 top-3(`_eligible` = 非ST∧roe>0∧bm>0∧流动性≥5e7;迟滞 δ 在 TRAIN 选;
  20bp)→ `to_index_relative` 指数相对超额。
- 报:**6 折均值超额 + 正折数 + OOS rank-IC**(打分 vs `fwd_ret_5d` 截面 Spearman)。
- 基线:ridge-on-gauss **+0.186 / 6-6 / IC≈0.066**(gauntlet ①)。判读:变体须 6 折正 + 胜基线 + 无单折依赖
  才算"有效应";否则记"无效应/证伪"。

## 四轴实验矩阵

### 轴1 逐因子归一化
- 归一化原语:`norm_rank` / `norm_gauss` / `norm_winz`(已存于 `test_norm_hysteresis`)。
- 变体:① 全 gauss(基线)② 全 rank ③ 全 winz ④ **逐因子按 TRAIN rank-IC 选 norm**(每因子在 train 上
  比 3 种 norm 的 |rank-IC|,取最高;train-only,不看 OOS)。
- 测:6 折超额 + IC。先验弱(归一化只在组合层起作用)。

### 轴2 dropout 数量(随机遮蔽因子)
- 在**原始 gauss 因子**上 bagging-ridge(非 PCA):B=20 袋,每袋随机遮蔽比例 p 的因子(列置零)后 `fit_ridge`,
  权重取袋均;p∈{0=基线, 0.25, 0.5, 0.75}。固定随机种子保可复现。
- 测:6 折超额 + IC vs p 曲线。先验:dropout-bagged ridge ≈ ridge。

### 轴3 权重取值区间(防单因子主导)
- 扫 ridge 权重 clip 分位:{p99 松, p90 基线, p75, p50 紧}(`fit_ridge` 的 clip 参数化)。
- 测:6 折超额 + **权重弥散度**(HHI = Σ(|wᵢ|/Σ|w|)²、最大单权占比)vs 约束。
- 先验:基线已弥散无主导;更紧多半过收缩伤 OOS、更松或现主导。量化权衡。

### 轴4 聚类→分模型(KMeans/GMM,无监督)
- 聚类:对 **TRAIN** 全周 gauss 因子矩阵 fit **numpy 手搓 KMeans**(Lloyd 迭代 + k-means++ 初始化,固定种子;
  遵循 repo "numpy/pandas only,无 sklearn" 约定)→ K 簇质心;前向把每个 OOS 截面的股票按最近质心分配
  (train 拟合、前向套用,无泄露)。K∈{2,3,5}。**GMM 不做(YAGNI;KMeans 足够刻画分模型效应)。**
- 模型:每簇用其 TRAIN 子样本 `fit_ridge` → 簇权重;OOS 打分时每股用**其所属簇**的权重,汇总全市场取 top-3。
- 测:6 折超额 vs 池化基线 + **过拟合护栏**:每簇 TRAIN 股×周样本量、每簇 train-vs-OOS rank-IC 落差、
  **簇分配跨期稳定性**(同股相邻周类别变动率)。GMM 软分配留待 KMeans 若意外有效再议。
- 先验:⚠️ 切 K 份 → 样本饥饿 → OOS 多半更差;正面量化"为何此样本下分模型不行"。

## 诚实护栏
- 一切对标 ridge 基线;"胜出"须过 §5.3(6 折正 + 胜基线 + 无单折依赖)。
- 轴4 必出过拟合三件套(样本量/IC 落差/簇稳定性)——这是本研究最有价值的诊断。
- 随机性固定种子;聚类/ dropout 可复现。

## 测试
- 纯函数单测(`scripts/test_train_ablation.py`,合成数据):per-factor-norm 选择器(train-IC 取最高)、
  dropout-bagging 权重均值形状/遮蔽正确、clip 分位单调改变弥散、KMeans 分配确定性 + 每簇 fit 形状。
- 不需真数据即可过的纯逻辑;真数据跑产出 findings。

## 交付
- `scripts/train_ablation.py` + `scripts/test_train_ablation.py` + `docs/superpowers/2026-06-24-ridge-ablation-findings.md`。
- 分支 `train-ablation`;纯 Python;引擎/部署/72 因子集/冻结权重均不动。

## 范围外(YAGNI)
- 不做监督式分类(易泄露)、不做 PCA 路线(已证更差)、不接桌面、不上实盘。
- 不把任何变体设为新部署候选(除非真过 §5.3,届时单独决策)。
