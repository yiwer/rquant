"""Pure-function tests for fetch_margin.py (no network, no akshare calls)."""
import pandas as pd
import pytest
import fetch_margin as fm


class TestToSymbol:
    """Tests for to_symbol_sse and to_symbol_szse."""

    def test_sse_str_code(self):
        assert fm.to_symbol_sse("600519") == "sh600519"

    def test_sse_int_code(self):
        assert fm.to_symbol_sse(600519) == "sh600519"

    def test_sse_short_code_zero_padded(self):
        # A short code must be zero-padded to 6 digits
        assert fm.to_symbol_sse("1234") == "sh001234"

    def test_sse_already_6_digits(self):
        assert fm.to_symbol_sse("000001") == "sh000001"

    def test_szse_str_code(self):
        assert fm.to_symbol_szse("000001") == "sz000001"

    def test_szse_int_code(self):
        assert fm.to_symbol_szse(1) == "sz000001"

    def test_szse_short_code_zero_padded(self):
        assert fm.to_symbol_szse("300") == "sz000300"

    def test_szse_already_6_digits(self):
        assert fm.to_symbol_szse("300750") == "sz300750"


class TestNormalizeSSE:
    """Tests for normalize_sse — synthetic 9-column SSE DataFrame."""

    def _make_sse_df(self):
        """Construct a synthetic SSE DataFrame matching the real column schema.

        Key design: rzye (融资余额) = 1_000_000 and rzmre (融资买入额) = 2_000_000
        so that any positional bug that swaps them is caught.
        """
        return pd.DataFrame({
            "信用交易日期":  ["20180102", "20180102"],
            "标的证券代码":  ["600519",   "601318"],
            "标的证券简称":  ["贵州茅台",  "中国平安"],
            "融资余额":     [1_000_000,  500_000],   # rzye — deliberately different
            "融资买入额":   [2_000_000,  300_000],   # rzmre
            "融资偿还额":   [100_000,    50_000],    # NOT extracted
            "融券余量":     [5_000,      3_000],     # rqyl
            "融券卖出量":   [1_000,      800],       # NOT extracted
            "融券偿还量":   [500,        200],       # NOT extracted
        })

    def test_symbol_prefix(self):
        out = fm.normalize_sse(self._make_sse_df())
        assert out.loc[0, "symbol"] == "sh600519"
        assert out.loc[1, "symbol"] == "sh601318"

    def test_date_normalized(self):
        out = fm.normalize_sse(self._make_sse_df())
        assert out.loc[0, "date"] == "2018-01-02"

    def test_rzye_correct_not_rzmre(self):
        """rzye should be 1_000_000, NOT 2_000_000 (catches positional swap)."""
        out = fm.normalize_sse(self._make_sse_df())
        assert out.loc[0, "rzye"] == 1_000_000
        assert out.loc[0, "rzmre"] == 2_000_000  # rzmre is the OTHER field

    def test_rqyl_correct(self):
        out = fm.normalize_sse(self._make_sse_df())
        assert out.loc[0, "rqyl"] == 5_000

    def test_output_columns_exactly(self):
        out = fm.normalize_sse(self._make_sse_df())
        assert list(out.columns) == ["symbol", "date", "rzye", "rzmre", "rqyl"]

    def test_row_count(self):
        out = fm.normalize_sse(self._make_sse_df())
        assert len(out) == 2

    def test_numerics_coerced(self):
        """Non-numeric value in 融资余额 should coerce to NaN, not raise.

        Build the DataFrame with 融资余额 as object dtype (mirrors real akshare
        output where columns may arrive as mixed/string type).
        """
        base = self._make_sse_df()
        # Reconstruct with 融资余额 as object column so we can mix in a string
        df = base.copy()
        df["融资余额"] = df["融资余额"].astype(object)
        df.loc[0, "融资余额"] = "N/A"
        out = fm.normalize_sse(df)
        assert pd.isna(out.loc[0, "rzye"])


class TestNormalizeSZSE:
    """Tests for normalize_szse — synthetic 8-column SZSE DataFrame.

    CRITICAL anti-regression: SZSE column ORDER is different from SSE.
    The function must select by name, not position.
    """

    def _make_szse_df(self):
        """Construct a synthetic SZSE DataFrame matching the real column schema.

        Column order: 证券代码, 证券简称, 融资买入额, 融资余额, 融券卖出量, 融券余量, 融券余额, 融资融券余额
        Note: 融资余额 is at position 3 (0-indexed), 融资买入额 at position 2 —
        swapped relative to SSE. A positional bug would assign rzmre to rzye.

        rzye (融资余额) = 3_000_000, rzmre (融资买入额) = 6_000_000 (distinct values).
        """
        return pd.DataFrame({
            "证券代码":     ["000001",   "300750"],
            "证券简称":     ["平安银行",  "宁德时代"],
            "融资买入额":   [6_000_000,  4_000_000],   # rzmre — position 2
            "融资余额":     [3_000_000,  2_000_000],   # rzye  — position 3 (different from SSE)
            "融券卖出量":   [8_000,      6_000],       # NOT extracted
            "融券余量":     [7_000,      5_000],       # rqyl  — position 5
            "融券余额":     [900_000,    700_000],     # NOT extracted
            "融资融券余额": [3_900_000,  2_700_000],   # NOT extracted
        })

    def test_symbol_prefix(self):
        out = fm.normalize_szse(self._make_szse_df(), "20180103")
        assert out.loc[0, "symbol"] == "sz000001"
        assert out.loc[1, "symbol"] == "sz300750"

    def test_date_from_param(self):
        """SZSE has no date column; date must come from the date_yyyymmdd param."""
        out = fm.normalize_szse(self._make_szse_df(), "20180103")
        assert out.loc[0, "date"] == "2018-01-03"
        assert out.loc[1, "date"] == "2018-01-03"

    def test_rzye_correct_not_rzmre(self):
        """rzye=3_000_000, rzmre=6_000_000 — different column order than SSE.

        A positional bug (using index instead of name) would assign 6_000_000 to rzye.
        """
        out = fm.normalize_szse(self._make_szse_df(), "20180103")
        assert out.loc[0, "rzye"] == 3_000_000
        assert out.loc[0, "rzmre"] == 6_000_000

    def test_rqyl_correct(self):
        out = fm.normalize_szse(self._make_szse_df(), "20180103")
        assert out.loc[0, "rqyl"] == 7_000

    def test_output_columns_exactly(self):
        out = fm.normalize_szse(self._make_szse_df(), "20180103")
        assert list(out.columns) == ["symbol", "date", "rzye", "rzmre", "rqyl"]

    def test_row_count(self):
        out = fm.normalize_szse(self._make_szse_df(), "20180103")
        assert len(out) == 2


class TestPivotToSymbol:
    """Tests for pivot_to_symbol — merging, sorting, deduplication."""

    def _make_rows(self):
        """Two dates for sh600519, one date for sz000001."""
        return pd.DataFrame({
            "symbol": ["sh600519", "sh600519", "sz000001"],
            "date":   ["2018-01-02", "2018-01-03", "2018-01-02"],
            "rzye":   [1_000_000,    1_100_000,    500_000],
            "rzmre":  [200_000,      220_000,      100_000],
            "rqyl":   [5_000,        5_500,        3_000],
        })

    def test_symbol_keys(self):
        result = fm.pivot_to_symbol(self._make_rows())
        assert "sh600519" in result
        assert "sz000001" in result

    def test_time_column_name(self):
        """Output column must be named 'time', not 'date'."""
        result = fm.pivot_to_symbol(self._make_rows())
        df = result["sh600519"]
        assert "time" in df.columns
        assert "date" not in df.columns

    def test_two_dates_for_symbol(self):
        result = fm.pivot_to_symbol(self._make_rows())
        df = result["sh600519"]
        assert len(df) == 2

    def test_ascending_sort(self):
        """Rows must be sorted ascending by time."""
        result = fm.pivot_to_symbol(self._make_rows())
        df = result["sh600519"]
        times = df["time"].tolist()
        assert times == sorted(times)
        assert times[0] == "2018-01-02"
        assert times[1] == "2018-01-03"

    def test_correct_values_after_sort(self):
        result = fm.pivot_to_symbol(self._make_rows())
        df = result["sh600519"].reset_index(drop=True)
        assert df.loc[0, "rzye"] == 1_000_000  # 2018-01-02
        assert df.loc[1, "rzye"] == 1_100_000  # 2018-01-03

    def test_dedup_keeps_last(self):
        """Duplicate time entries: keep-last policy."""
        rows = pd.DataFrame({
            "symbol": ["sh600519", "sh600519"],
            "date":   ["2018-01-02", "2018-01-02"],   # duplicate date
            "rzye":   [1_000_000,    9_999_999],       # second value should win
            "rzmre":  [200_000,      999_000],
            "rqyl":   [5_000,        9_000],
        })
        result = fm.pivot_to_symbol(rows)
        df = result["sh600519"]
        assert len(df) == 1
        assert df.iloc[0]["rzye"] == 9_999_999

    def test_empty_input(self):
        result = fm.pivot_to_symbol(pd.DataFrame(
            columns=["symbol", "date", "rzye", "rzmre", "rqyl"]
        ))
        assert result == {}

    def test_output_columns(self):
        result = fm.pivot_to_symbol(self._make_rows())
        df = result["sh600519"]
        assert list(df.columns) == ["time", "rzye", "rzmre", "rqyl"]
