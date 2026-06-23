"""Pure-function tests for fetch_financials_extra.py (no network, no akshare calls)."""
import pandas as pd
import pytest
import fetch_financials_extra as ff


class TestPeriodToDisclosure:
    def test_q1_same_year_apr30(self):
        assert ff.period_to_disclosure("20230331") == "2023-04-30"

    def test_q2_same_year_aug31(self):
        assert ff.period_to_disclosure("20230630") == "2023-08-31"

    def test_q3_same_year_oct31(self):
        assert ff.period_to_disclosure("20230930") == "2023-10-31"

    def test_q4_next_year_apr30(self):
        assert ff.period_to_disclosure("20231231") == "2024-04-30"

    def test_q4_year_boundary(self):
        # 2018 year-end → 2019-04-30
        assert ff.period_to_disclosure("20181231") == "2019-04-30"

    def test_q1_another_year(self):
        assert ff.period_to_disclosure("20180331") == "2018-04-30"


class TestExtractSeries:
    def _make_fa(self):
        """Minimal DataFrame mimicking stock_financial_abstract output."""
        return pd.DataFrame({
            "指标":    ["资产负债率", "投入资本回报率", "噪声行",   "经营现金流量净额"],
            "20231231": ["55.5",      "8.2",           "x",        "1234567"],
            "20230930": ["54.0",      "7.1",           "y",        "999000"],
            "20170930": ["50.0",      "6.0",           "z",        "111000"],  # pre-2018, should be skipped
        })

    def test_picks_correct_indicators(self):
        out = ff.extract_series(self._make_fa(), from_year=2018)
        # 20231231 → 2024-04-30
        row = out["2024-04-30"]
        assert abs(float(row["debt_ratio"]) - 55.5) < 1e-9
        assert abs(float(row["roic"]) - 8.2) < 1e-9
        assert abs(float(row["cfo"]) - 1234567.0) < 1e-9

    def test_pit_date_keys_correct(self):
        out = ff.extract_series(self._make_fa(), from_year=2018)
        assert "2024-04-30" in out   # 20231231
        assert "2023-10-31" in out   # 20230930

    def test_pre_from_year_excluded(self):
        out = ff.extract_series(self._make_fa(), from_year=2018)
        # 20170930 → 2017-10-31, should not appear
        assert "2017-10-31" not in out

    def test_missing_indicator_gives_empty_string(self):
        out = ff.extract_series(self._make_fa(), from_year=2018)
        row = out["2024-04-30"]
        # net_margin not present in the mini DataFrame
        assert row["net_margin"] == ""

    def test_first_matching_row_used(self):
        """When 指标 appears multiple times, the first row wins."""
        fa = pd.DataFrame({
            "指标":    ["资产负债率", "资产负债率"],
            "20231231": ["55.5",      "99.9"],
        })
        out = ff.extract_series(fa, from_year=2018)
        assert abs(float(out["2024-04-30"]["debt_ratio"]) - 55.5) < 1e-9

    def test_nan_value_becomes_empty_string(self):
        import math
        fa = pd.DataFrame({
            "指标":    ["资产负债率"],
            "20231231": [float("nan")],
        })
        out = ff.extract_series(fa, from_year=2018)
        assert out["2024-04-30"]["debt_ratio"] == ""

    def test_out_cols_all_present(self):
        out = ff.extract_series(self._make_fa(), from_year=2018)
        for col in ff.OUT_COLS:
            assert col in out["2024-04-30"], f"Missing column: {col}"

    # --- Canonical assertions from the spec ---
    def test_spec_canonical_debt_ratio(self):
        fa = pd.DataFrame({
            "指标":    ["资产负债率", "投入资本回报率", "噪声行"],
            "20231231": ["55.5",      "8.2",           "x"],
            "20230930": ["54.0",      "7.1",           "y"],
        })
        out = ff.extract_series(fa, from_year=2018)
        v = out["2024-04-30"]["debt_ratio"]
        assert v == 55.5 or v == "55.5" or abs(float(v) - 55.5) < 1e-9

    def test_spec_canonical_q3_roic_present(self):
        fa = pd.DataFrame({
            "指标":    ["资产负债率", "投入资本回报率", "噪声行"],
            "20231231": ["55.5",      "8.2",           "x"],
            "20230930": ["54.0",      "7.1",           "y"],
        })
        out = ff.extract_series(fa, from_year=2018)
        assert "2023-10-31" in out
        assert "roic" in out["2023-10-31"]
