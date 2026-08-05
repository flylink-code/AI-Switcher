import type { ReactNode } from "react";
import { Col, Statistic } from "antd";
import { formatCompactNumber } from "@/utils/formatCompact";

export function UsageMetric({
  title,
  value,
  suffix,
  prefix,
  precision,
  icon,
  compact,
}: {
  title: string;
  value: number;
  suffix?: string;
  prefix?: string;
  precision?: number;
  icon?: ReactNode;
  compact?: boolean;
}) {
  return (
    <Col xs={24} sm={12} xl={6}>
      <div className="usage-metric-tile">
        <Statistic
          title={title}
          value={value}
          suffix={suffix}
          prefix={prefix ?? icon}
          precision={precision}
          formatter={compact ? (v) => formatCompactNumber(Number(v)) : undefined}
        />
      </div>
    </Col>
  );
}
