import { JsonView, collapseAllNested } from "react-json-view-lite";

// The library exposes CSS icon slots only. Their masks use the bundled Lucide
// ChevronRight SVG (rotated for collapse), preserving its native tree behavior.
const jsonTreeStyles = {
  container: "app-json-tree",
  childFieldsContainer: "app-json-tree-children",
  basicChildStyle: "app-json-tree-child",
  collapseIcon: "app-json-tree-toggle app-json-tree-collapse",
  expandIcon: "app-json-tree-toggle app-json-tree-expand",
  collapsedContent: "app-json-tree-collapsed",
  label: "app-json-tree-label",
  clickableLabel: "app-json-tree-label app-json-tree-label-clickable",
  nullValue: "app-json-tree-null",
  undefinedValue: "app-json-tree-null",
  numberValue: "app-json-tree-number",
  stringValue: "app-json-tree-string",
  booleanValue: "app-json-tree-boolean",
  otherValue: "app-json-tree-other",
  punctuation: "app-json-tree-punctuation",
  quotesForFieldNames: true,
  stringifyStringValues: true,
  ariaLables: {
    collapseJson: "折叠 JSON 节点",
    expandJson: "展开 JSON 节点",
  },
};

interface JsonTreeViewProps {
  data: object | unknown[];
  ariaLabel: string;
}

export function JsonTreeView({ data, ariaLabel }: JsonTreeViewProps) {
  return (
    <JsonView
      aria-label={ariaLabel}
      data={data}
      style={jsonTreeStyles}
      shouldExpandNode={collapseAllNested}
      clickToExpandNode
      compactTopLevel
    />
  );
}
