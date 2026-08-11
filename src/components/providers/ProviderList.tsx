import React from "react";
import type { Provider } from "@/types/backend";
import { ProviderListItem } from "./ProviderListItem";

export interface ProviderListProps {
  providers: Provider[];
  selectedId: string | null;
  onSelect: (provider: Provider) => void;
  className?: string;
  style?: React.CSSProperties;
}

/** Scrollable master list of providers (left pane of the workspace). */
export const ProviderList: React.FC<ProviderListProps> = ({
  providers,
  selectedId,
  onSelect,
  className = "",
  style,
}) => {
  return (
    <div
      className={className}
      role="listbox"
      aria-label="Providers"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 2,
        overflowY: "auto",
        minHeight: 0,
        ...style,
      }}
    >
      {providers.map((provider) => (
        <ProviderListItem
          key={provider.id}
          provider={provider}
          selected={provider.id === selectedId}
          onSelect={onSelect}
        />
      ))}
    </div>
  );
};
