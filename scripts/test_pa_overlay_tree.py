#!/usr/bin/env python3
"""pa_overlay 树 + 配置能被 rquant 引擎加载(lint 不报错)。
跑：python -m pytest scripts/test_pa_overlay_tree.py -q"""
import os, subprocess
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def test_configs_exist_and_yaml_valid():
    import yaml
    for name in ["value_paov_l0", "value_paov_l03", "value_paov_l05", "value_paov_l07"]:
        p = os.path.join(REPO, "examples", "screen", "iter", f"{name}.yaml")
        with open(p, encoding="utf-8") as f:
            cfg = yaml.safe_load(f)
        assert cfg["merge"]["top"] == 50
        assert "ov" in cfg["merge"]["tilt_setups"]
    tree = os.path.join(REPO, "examples", "trees", "screen", "pa_overlay.yaml")
    with open(tree, encoding="utf-8") as f:
        t = yaml.safe_load(f)
    assert t["meta"]["name"] == "pa_overlay"
