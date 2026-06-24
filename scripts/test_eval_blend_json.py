import json, os, tempfile, subprocess, sys
def test_blend_json_shape(tmp_path=None):
    # 烟囱测试:跑 eval_blend.py --json 到临时文件,断言键齐全、folds 非空
    out = os.path.join(tempfile.gettempdir(), "blend_test.json")
    if os.path.exists(out): os.remove(out)
    r = subprocess.run([sys.executable, "scripts/eval_blend.py", "--json", out],
                       capture_output=True, text=True, timeout=900)
    assert r.returncode == 0, r.stderr[-2000:]
    d = json.load(open(out, encoding="utf-8"))
    assert "folds" in d and "mean" in d and len(d["folds"]) >= 4
    keys = {"corr","sh_ridge","sh_val","sh_blend","dd_ridge","dd_val","dd_blend","ex_ridge","ex_val","ex_blend"}
    assert keys <= set(d["mean"]) and keys | {"oos"} <= set(d["folds"][0])

if __name__ == "__main__":
    test_blend_json_shape(); print("PASS")
