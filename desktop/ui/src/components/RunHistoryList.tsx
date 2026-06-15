import { Checkbox, List, Popconfirm, Tag, Typography } from "antd";
import type { RunMetaDto } from "@bindings/RunMetaDto";
import { modeZh } from "../labels";

const KIND_TAG: Record<string, string> = {
  sim_hard: "blue", sim_soft: "geekblue", score_hard: "purple", score_soft: "magenta",
};

export default function RunHistoryList({
  runs, selectedId, compareIds, onSelect, onToggleCompare, onDelete,
}: {
  runs: RunMetaDto[];
  selectedId: string | null;
  compareIds: string[];
  onSelect: (id: string) => void;
  onToggleCompare: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  return (
    <List
      size="small"
      dataSource={runs}
      locale={{ emptyText: "暂无留档——跑一次回测吧" }}
      renderItem={(r) => (
        <List.Item
          style={{ cursor: "pointer", background: r.id === selectedId ? "rgba(22,119,255,.08)" : undefined }}
          onClick={() => onSelect(r.id)}
          actions={[
            <Checkbox
              key="c"
              checked={compareIds.includes(r.id)}
              onClick={(e) => e.stopPropagation()}
              onChange={() => onToggleCompare(r.id)}
            >
              对比
            </Checkbox>,
            <Popconfirm key="d" title="删除该留档?" onConfirm={() => onDelete(r.id)}>
              <Typography.Link onClick={(e) => e.stopPropagation()}>删除</Typography.Link>
            </Popconfirm>,
          ]}
        >
          <List.Item.Meta
            title={
              <>
                <Tag color={r.ok ? KIND_TAG[r.kind] ?? "default" : "red"}>{r.ok ? modeZh(r.kind) : "失败"}</Tag>
                {r.name}
              </>
            }
            description={`${r.id} · ${r.created}`}
          />
        </List.Item>
      )}
    />
  );
}
