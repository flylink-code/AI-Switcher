import { useEffect, useRef, useState } from "react";
import { Alert, Button, Form, InputNumber, Modal, Space, Spin, message } from "antd";
import { useTranslation } from "react-i18next";
import type { UpstreamLimitPolicy } from "@/types/backend";
import { getGatewayUpstreamPolicy, setGatewayUpstreamPolicy } from "@/services/providers";

export interface UpstreamLimitsModalProps {
  open: boolean;
  upstreamId: string | null;
  upstreamName: string;
  onClose: () => void;
}

const DEFAULT_LIMIT_POLICY: UpstreamLimitPolicy = {
  maxConcurrency: 0,
  rpm: 0,
  queueCapacity: 16,
  queueTimeoutMs: 8000,
  firstOutputTimeoutMs: 0,
};
const FIELDS = [
  { name: "maxConcurrency", max: 64, step: 1 },
  { name: "rpm", max: 6000, step: 1 },
  { name: "queueCapacity", max: 256, step: 1 },
  { name: "queueTimeoutMs", max: 120_000, step: 500 },
  { name: "firstOutputTimeoutMs", max: 300_000, step: 1000 },
] as const;

export function UpstreamLimitsModal({ open, upstreamId, upstreamName, onClose }: UpstreamLimitsModalProps) {
  const { t } = useTranslation();
  const [form] = Form.useForm<UpstreamLimitPolicy>();
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const requestId = useRef(0);
  const savingRef = useRef(false);

  useEffect(() => {
    const generation = ++requestId.current;
    setLoaded(false);
    if (!open || !upstreamId) return;
    setLoading(true);
    form.resetFields();
    void getGatewayUpstreamPolicy(upstreamId).then((policy) => {
      if (generation !== requestId.current) return;
      form.setFieldsValue(policy);
      setLoaded(true);
    }).catch((error: unknown) => {
      if (generation === requestId.current) {
        void message.error(t("upstreamLimits.loadFailed", { error: String(error) }));
      }
    }).finally(() => {
      if (generation === requestId.current) setLoading(false);
    });
    return () => { requestId.current = generation + 1; };
  }, [open, upstreamId, form, t]);

  const save = async () => {
    if (!upstreamId || !loaded || loading || savingRef.current) return;
    const generation = requestId.current;
    savingRef.current = true;
    setSaving(true);
    try {
      const values = await form.validateFields();
      if (generation !== requestId.current) return;
      await setGatewayUpstreamPolicy(upstreamId, values);
      if (generation === requestId.current) {
        void message.success(t("upstreamLimits.saveSuccess"));
        onClose();
      }
    } catch (error) {
      if (generation === requestId.current && !(error && typeof error === "object" && "errorFields" in error)) {
        void message.error(t("upstreamLimits.saveFailed", { error: String(error) }));
      }
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  return (
    <Modal
      open={open}
      title={t("upstreamLimits.modalTitle", { name: upstreamName || upstreamId || "" })}
      onCancel={() => { if (!savingRef.current) onClose(); }}
      destroyOnHidden
      width={640}
      maskClosable={!saving}
      closable={!saving}
      keyboard={!saving}
      footer={
        <Space wrap style={{ display: "flex", justifyContent: "space-between" }}>
          <Button onClick={() => form.setFieldsValue(DEFAULT_LIMIT_POLICY)} disabled={!loaded || loading || saving}>
            {t("upstreamLimits.resetDefaults")}
          </Button>
          <Space>
            <Button onClick={onClose} disabled={saving}>{t("common.cancel")}</Button>
            <Button type="primary" loading={saving} disabled={!loaded || loading} onClick={() => void save()}>
              {t("common.save")}
            </Button>
          </Space>
        </Space>
      }
    >
      <Alert
        type="info"
        showIcon
        title={t("upstreamLimits.noticeTitle")}
        description={
          <Space orientation="vertical" size={4}>
            <div>{t("upstreamLimits.scopeNotice")}</div>
            <div>{t("upstreamLimits.zeroMeaningNotice")}</div>
            <div>{t("upstreamLimits.timeoutSideEffectNotice")}</div>
          </Space>
        }
        style={{ marginBottom: 16 }}
      />
      <Spin spinning={loading}>
        <Form form={form} layout="vertical" initialValues={DEFAULT_LIMIT_POLICY} disabled={!loaded || loading || saving}>
          {FIELDS.map(({ name, max, step }) => (
            <Form.Item
              key={name}
              name={name}
              label={t(`upstreamLimits.${name}Label`)}
              extra={t(`upstreamLimits.${name}Help`)}
              rules={[
                { required: true, message: t(`upstreamLimits.${name}Required`) },
                { type: "integer", min: 0, max, message: t(`upstreamLimits.${name}Range`) },
              ]}
            >
              <InputNumber min={0} max={max} step={step} precision={0} style={{ width: "100%" }} />
            </Form.Item>
          ))}
        </Form>
      </Spin>
    </Modal>
  );
}
