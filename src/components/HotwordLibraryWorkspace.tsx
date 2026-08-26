import { AlertTriangle, Plus, Save, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type {
  HotwordEntry,
  HotwordListDocument,
  HotwordListSummary,
  HotwordNormalization,
} from "../types";

interface HotwordLibraryWorkspaceProps {
  lists: HotwordListSummary[];
  onRefresh: () => Promise<void>;
  onDirtyChange: (dirty: boolean) => void;
  onMessage: (type: "success" | "error", message: string) => void;
}

const draftDocument = (): HotwordListDocument => ({
  id: "",
  name: "新建热词列表",
  entries: [],
  createdAt: "",
  updatedAt: "",
});

const escapeHotwordText = (value: string) => value
  .replaceAll("\\", "\\\\")
  .replaceAll(":", "\\:")
  .replaceAll("：", "\\：");

const formatEntry = (entry: HotwordEntry) => {
  const text = escapeHotwordText(entry.text);
  return entry.weight === null ? text : `${text}:${entry.weight}`;
};

const formatContent = (entries: HotwordEntry[]) => entries.map(formatEntry).join("\n");

const previewLine = (line: string) => {
  const value = line.trim();
  let escaped = false;
  let delimiter = -1;
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (escaped) {
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (character === ":" || character === "：") {
      delimiter = index;
    }
  }
  if (delimiter < 0) return { text: value, weight: null };
  const source = value.slice(delimiter + 1).trim();
  const parsed = Number(source);
  const weight = !source || source === "0"
    ? null
    : Number.isInteger(parsed) && ([1, 2, 3, 4, 5, 50].includes(parsed))
      ? parsed
      : 4;
  return { text: value.slice(0, delimiter).trim(), weight };
};

export function HotwordLibraryWorkspace(props: HotwordLibraryWorkspaceProps) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [document, setDocument] = useState<HotwordListDocument | null>(null);
  const [name, setName] = useState("");
  const [content, setContent] = useState("");
  const [savedSignature, setSavedSignature] = useState("");
  const [normalizations, setNormalizations] = useState<HotwordNormalization[]>([]);
  const [busy, setBusy] = useState(false);

  const signature = `${name}\n${content}`;
  const dirty = document !== null && signature !== savedSignature;
  useEffect(() => props.onDirtyChange(dirty), [dirty, props.onDirtyChange]);

  const counts = useMemo(() => {
    const values = content.split(/\r?\n/).map(previewLine).filter((entry) => entry.text);
    const unique = new Map<string, number | null>();
    values.forEach((entry) => {
      if (!unique.has(entry.text)) unique.set(entry.text, entry.weight);
    });
    return {
      effective: unique.size,
      duplicates: values.length - unique.size,
      weighted: [...unique.values()].filter((weight) => weight !== null).length,
      superHotwords: [...unique.values()].filter((weight) => weight === 50).length,
    };
  }, [content]);

  const load = async (id: string) => {
    if (dirty && !confirm("热词列表尚未保存。放弃这些更改吗？")) return;
    setBusy(true);
    try {
      const next = await api.getHotwordList(id);
      const nextContent = formatContent(next.entries);
      setSelectedId(id);
      setDocument(next);
      setName(next.name);
      setContent(nextContent);
      setSavedSignature(`${next.name}\n${nextContent}`);
      setNormalizations([]);
    } catch (error) {
      props.onMessage("error", String(error));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (document || props.lists.length === 0) return;
    void load(props.lists[0].id);
    // The initial selection is intentionally driven only by the refreshed list.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.lists, document]);

  const create = () => {
    if (dirty && !confirm("热词列表尚未保存。放弃这些更改吗？")) return;
    const next = draftDocument();
    setSelectedId(null);
    setDocument(next);
    setName(next.name);
    setContent("");
    setSavedSignature("");
    setNormalizations([]);
  };

  const save = async () => {
    setBusy(true);
    try {
      const result = await api.saveHotwordList({
        id: document?.id || null,
        name,
        content,
      });
      const saved = result.document;
      const nextContent = formatContent(saved.entries);
      setSelectedId(saved.id);
      setDocument(saved);
      setName(saved.name);
      setContent(nextContent);
      setSavedSignature(`${saved.name}\n${nextContent}`);
      setNormalizations(result.normalizations);
      await props.onRefresh();
      props.onMessage(
        "success",
        result.normalizations.length
          ? `热词列表已保存，并规范化 ${result.normalizations.length} 处输入`
          : "热词列表已保存",
      );
    } catch (error) {
      props.onMessage("error", String(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!document?.id || !confirm(`删除“${document.name}”？历史转写中的热词快照不会受影响。`)) return;
    setBusy(true);
    try {
      await api.deleteHotwordList(document.id);
      setDocument(null);
      setSelectedId(null);
      setName("");
      setContent("");
      setSavedSignature("");
      setNormalizations([]);
      await props.onRefresh();
      props.onMessage("success", "热词列表已删除");
    } catch (error) {
      props.onMessage("error", String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="hotword-workspace">
      <aside className="hotword-list-pane">
        <div className="hotword-pane-header">
          <div><strong>热词列表</strong><span>{props.lists.length} 份</span></div>
          <button className="button secondary compact" onClick={create}><Plus size={15} />新建</button>
        </div>
        <div className="hotword-list-items">
          {props.lists.map((list) => (
            <button
              key={list.id}
              className={selectedId === list.id ? "active" : ""}
              onClick={() => void load(list.id)}
            >
              <strong>{list.name}</strong>
              <span>
                {list.entryCount} 个词
                {list.superHotwordCount > 0 ? ` · ${list.superHotwordCount} 个超级热词` : ""}
              </span>
            </button>
          ))}
          {props.lists.length === 0 && !document && (
            <p>还没有热词列表。为不同会议场景创建一份吧。</p>
          )}
        </div>
      </aside>

      <div className="hotword-editor-pane">
        {document ? (
          <>
            <header>
              <div><p className="eyebrow">LOCAL HOTWORD LIBRARY</p><h2>{document.id ? "编辑热词列表" : "创建热词列表"}</h2></div>
              <div className="hotword-editor-actions">
                {document.id && <button className="button secondary compact" disabled={busy} onClick={() => void remove()}><Trash2 size={15} />删除</button>}
                <button className="button primary compact" disabled={busy || !dirty} onClick={() => void save()}><Save size={15} />保存</button>
              </div>
            </header>
            <label className="hotword-name-field">
              <span>列表名称</span>
              <input value={name} maxLength={80} onChange={(event) => setName(event.target.value)} />
            </label>
            <label className="hotword-content-field">
              <span>热词与短语</span>
              <textarea
                aria-label="热词与短语"
                value={content}
                spellCheck={false}
                placeholder={"例如：\nBusabase:50\nNota\n产品名称：5"}
                onChange={(event) => {
                  setContent(event.target.value);
                  setNormalizations([]);
                }}
              />
            </label>
            <p className="hotword-help">
              每行填写一个热词或短语。可在末尾使用“:权重”，例如 Busabase:50；支持中英文冒号。不填写、填写 :0 或空值时使用 Provider 默认权重。千问支持 1–5 和超级热词 50，其他 Provider 可能忽略权重。热词正文中的冒号请写成 \:。
            </p>
            <div className="hotword-metrics">
              <span>有效词条 <strong>{counts.effective}</strong></span>
              <span>重复项 <strong>{counts.duplicates}</strong></span>
              <span>显式权重 <strong>{counts.weighted}</strong></span>
              <span>超级热词 <strong>{counts.superHotwords}</strong></span>
              <span>上限 2,000 条 · 单条 100 字符</span>
            </div>
            {normalizations.length > 0 && (
              <div className="hotword-compatibility">
                <AlertTriangle size={17} />
                <span>{normalizations.map((item) => item.message).join("；")}</span>
              </div>
            )}
            <div className="hotword-compatibility"><AlertTriangle size={17} /><span>保存采用通用规则；开始转写时还会按当前 Provider 和模型严格校验。SenseVoice 与 OpenAI-compatible 不支持热词。</span></div>
          </>
        ) : (
          <div className="hotword-empty"><strong>选择或创建一份热词列表</strong><span>热词库仅保存在本机，可供支持热词的不同转写服务复用。</span></div>
        )}
      </div>
    </section>
  );
}
