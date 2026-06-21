#!/usr/bin/env python3
"""3 策略配置 + 2 新树 YAML 合法 + 关键字段。
跑：python -m pytest scripts/test_top3_strategy_configs.py -q"""
import os, yaml
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _load(rel):
    with open(os.path.join(REPO, rel), encoding="utf-8") as f:
        return yaml.safe_load(f)


def test_three_configs_top3():
    for name in ["s1_value_top3", "s2_value_pa1h_top3", "s3_sector_value_top3"]:
        c = _load(f"examples/screen/iter/{name}.yaml")
        assert c["merge"]["top"] == 3
        assert len(c["quality_trees"]) == 3       # 三核
    s2 = _load("examples/screen/iter/s2_value_pa1h_top3.yaml")
    assert s2["value_frac"] == 0.03 and s2["merge"]["lambda"] == 1.5
    assert "pa" in s2["merge"]["tilt_setups"]
    s3 = _load("examples/screen/iter/s3_sector_value_top3.yaml")
    assert "sec" in s3["merge"]["tilt_setups"]


def test_two_new_trees():
    assert _load("examples/trees/screen/pa1h_overlay.yaml")["meta"]["name"] == "pa1h_overlay"
    assert _load("examples/trees/screen/sector_strength.yaml")["meta"]["name"] == "sector_strength"
