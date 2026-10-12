import { createContext, useContext, useMemo, type CSSProperties, type HTMLAttributes } from "react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import HolderOutlined from "@ant-design/icons/es/icons/HolderOutlined";
import { Button } from "antd";

interface DragHandleContextValue {
  setActivatorNodeRef?: (element: HTMLElement | null) => void;
  listeners?: ReturnType<typeof useSortable>["listeners"];
  attributes?: ReturnType<typeof useSortable>["attributes"];
}

const DragHandleContext = createContext<DragHandleContextValue>({});

interface SortableRowProps extends HTMLAttributes<HTMLTableRowElement> {
  "data-row-key": string;
}

/** antd `Table` body row that can be reordered with a drag handle. */
export function SortableRow({ style, children, ...props }: SortableRowProps) {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, transition, isDragging } =
    useSortable({ id: props["data-row-key"] });

  const rowStyle: CSSProperties = {
    ...style,
    transform: CSS.Translate.toString(transform),
    transition,
    ...(isDragging ? { position: "relative", zIndex: 9999 } : null),
  };
  const context = useMemo<DragHandleContextValue>(
    () => ({ setActivatorNodeRef, listeners, attributes }),
    [setActivatorNodeRef, listeners, attributes],
  );

  return (
    <DragHandleContext.Provider value={context}>
      <tr {...props} ref={setNodeRef} style={rowStyle}>
        {children}
      </tr>
    </DragHandleContext.Provider>
  );
}

export function DragHandle({ label, disabled }: { label: string; disabled?: boolean }) {
  const { setActivatorNodeRef, listeners, attributes } = useContext(DragHandleContext);
  return (
    <Button
      type="text"
      size="small"
      aria-label={label}
      disabled={disabled}
      icon={<HolderOutlined />}
      style={{ cursor: disabled ? "not-allowed" : "grab", touchAction: "none" }}
      ref={setActivatorNodeRef}
      {...attributes}
      {...listeners}
    />
  );
}
