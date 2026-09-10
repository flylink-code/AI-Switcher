import { Button, Collapse, Drawer, Table, Tabs, Typography } from "antd";
import BookOutlined from "@ant-design/icons/es/icons/BookOutlined";
import QuestionCircleOutlined from "@ant-design/icons/es/icons/QuestionCircleOutlined";
import { useTranslation } from "react-i18next";

const { Paragraph, Text, Title } = Typography;

export type RouteHelpTab = "guide" | "tutorial";

const MODE_HELP_IDS = [
  "image_gen",
  "web_search",
  "vision",
  "long_context",
  "background",
  "plan",
  "think",
  "edit",
  "default",
] as const;

type RecipeOn = "on" | "off" | "optional";

type RecipeRow = {
  modeId: (typeof MODE_HELP_IDS)[number];
  on: RecipeOn;
  pickKey: string;
  extraKey: string;
};

const RECIPE_MINIMAL: RecipeRow[] = [
  { modeId: "default", on: "on", pickKey: "tutorialPickDaily", extraKey: "tutorialEffortOff" },
  { modeId: "background", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialDash" },
  { modeId: "plan", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialDash" },
  { modeId: "think", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialDash" },
  { modeId: "edit", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialDash" },
  { modeId: "long_context", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialDash" },
  { modeId: "web_search", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialDash" },
  { modeId: "vision", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialDash" },
  { modeId: "image_gen", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialDash" },
];

const RECIPE_CCR: RecipeRow[] = [
  { modeId: "default", on: "on", pickKey: "tutorialPickDaily", extraKey: "tutorialEffortOff" },
  { modeId: "background", on: "on", pickKey: "tutorialPickGeminiFlash", extraKey: "tutorialEffortOff" },
  { modeId: "think", on: "on", pickKey: "tutorialPickAstra", extraKey: "tutorialEffortHigh" },
  { modeId: "long_context", on: "on", pickKey: "tutorialPickGeminiLong", extraKey: "tutorialThreshold60k" },
  { modeId: "plan", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialKeepOff" },
  { modeId: "edit", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialKeepOff" },
  { modeId: "web_search", on: "optional", pickKey: "tutorialPickGeminiSearch", extraKey: "tutorialEffortOff" },
  { modeId: "vision", on: "optional", pickKey: "tutorialPickGeminiVision", extraKey: "tutorialEffortOff" },
  { modeId: "image_gen", on: "optional", pickKey: "tutorialPickImagen", extraKey: "tutorialEffortOff" },
];

const RECIPE_MIX: RecipeRow[] = [
  { modeId: "default", on: "on", pickKey: "tutorialPickDeepSeek", extraKey: "tutorialEffortOff" },
  { modeId: "background", on: "on", pickKey: "tutorialPickGeminiFlash", extraKey: "tutorialEffortOff" },
  { modeId: "think", on: "on", pickKey: "tutorialPickAstra", extraKey: "tutorialEffortHigh" },
  { modeId: "long_context", on: "on", pickKey: "tutorialPickGeminiLong", extraKey: "tutorialThreshold60k" },
  { modeId: "vision", on: "on", pickKey: "tutorialPickGeminiVision", extraKey: "tutorialEffortOff" },
  { modeId: "web_search", on: "optional", pickKey: "tutorialPickGeminiSearch", extraKey: "tutorialEffortOff" },
  { modeId: "image_gen", on: "optional", pickKey: "tutorialPickImagen", extraKey: "tutorialEffortOff" },
  { modeId: "plan", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialKeepOff" },
  { modeId: "edit", on: "off", pickKey: "tutorialPickEmpty", extraKey: "tutorialKeepOff" },
];

const TUTORIAL_AGENTS = [
  "claude_code",
  "claude_desktop",
  "codex",
  "opencode",
  "pi",
  "dsh",
  "cline",
  "custom",
] as const;

type TutorialAgentId = (typeof TUTORIAL_AGENTS)[number];

const AGENT_STEP_KEYS = ["bind", "model", "modes", "recipe", "check"] as const;

export function RouteModesHelpButton({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation();
  return (
    <Button size="small" icon={<QuestionCircleOutlined />} onClick={onClick}>
      {t("gateway.modesHelp", { defaultValue: "说明" })}
    </Button>
  );
}

export function RouteModesTutorialButton({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation();
  return (
    <Button size="small" icon={<BookOutlined />} onClick={onClick}>
      {t("gateway.modesTutorial", { defaultValue: "教程" })}
    </Button>
  );
}

export function RouteModesHelpDrawer({
  open,
  onClose,
  tab,
  onTabChange,
}: {
  open: boolean;
  onClose: () => void;
  tab: RouteHelpTab;
  onTabChange: (tab: RouteHelpTab) => void;
}) {
  const { t } = useTranslation();
  return (
    <Drawer
      title={t("gateway.modesHelpTitle", { defaultValue: "路由模式怎么配" })}
      open={open}
      onClose={onClose}
      size="large"
      destroyOnClose
    >
      <Tabs
        activeKey={tab}
        onChange={(key) => onTabChange(key === "tutorial" ? "tutorial" : "guide")}
        items={[
          {
            key: "guide",
            label: t("gateway.modesHelpTabGuide", { defaultValue: "说明" }),
            children: <GuidePanel />,
          },
          {
            key: "tutorial",
            label: t("gateway.modesHelpTabTutorial", { defaultValue: "教程" }),
            children: <TutorialPanel />,
          },
        ]}
      />
    </Drawer>
  );
}

function GuidePanel() {
  const { t } = useTranslation();
  return (
    <>
      <Paragraph>
        {t("gateway.modesHelpIntro", {
          defaultValue:
            "Agent 请求 auto 时，网关按这次请求的特征选一行。你在 /model 里点了具体目录模型时，模式全部让路。",
        })}
      </Paragraph>

      <Title level={5}>{t("gateway.modesHelpOrderTitle", { defaultValue: "匹配顺序（先到先得）" })}</Title>
      <Paragraph>
        {t("gateway.modesHelpOrderBody", {
          defaultValue:
            "显式目录模型 → 条件规则 → 图像生成 → 联网 → 视觉 → 长上下文 → 后台 → 规划 → 思考 → 改内容 → 默认。关掉或没选模型的行会被跳过。",
        })}
      </Paragraph>

      <Title level={5}>{t("gateway.modesHelpWhenTitle", { defaultValue: "各模式何时触发" })}</Title>
      <ul style={{ paddingLeft: 20, marginBottom: 16 }}>
        {MODE_HELP_IDS.map((id) => (
          <li key={id} style={{ marginBottom: 8 }}>
            <Text strong>{t(`gateway.modes.${id}`, { defaultValue: id })}</Text>
            <span>{" — "}</span>
            <Text type="secondary">{t(`gateway.modeHint.${id}`, { defaultValue: id })}</Text>
          </li>
        ))}
      </ul>

      <Title level={5}>{t("gateway.modesHelpColsTitle", { defaultValue: "每一列" })}</Title>
      <ul style={{ paddingLeft: 20, marginBottom: 16 }}>
        <li>{t("gateway.modesHelpColEnabled", { defaultValue: "启用：关 = 这行不参与匹配。" })}</li>
        <li>{t("gateway.modesHelpColModel", { defaultValue: "模型：命中后真正去打的上游，可跨供应商。" })}</li>
        <li>{t("gateway.modesHelpColThinking", { defaultValue: "挡位：思考强度。关闭最省；规划/思考常用高。" })}</li>
        <li>{t("gateway.modesHelpColFallback", { defaultValue: "备用：这行失败时最多再试 3 个模型。" })}</li>
        <li>{t("gateway.modesHelpColThreshold", { defaultValue: "阈值：仅长上下文。估算 token 超过才换模型。" })}</li>
      </ul>

      <Title level={5}>{t("gateway.modesHelpCcrTitle", { defaultValue: "和 Claude Code Router 的对应" })}</Title>
      <Paragraph>
        {t("gateway.modesHelpCcrBody", {
          defaultValue:
            "经典 CCR 在 JSON 的 Router 里写 default / background / think / longContext（后来还有 webSearch、image）。新版 CCR 改成规则列表（model-prefix / condition / script）加失败兜底。这里把常用槽做成 9 行固定表，不引入 JS 脚本或小模型分类器；规划/改内容只看工具名，不看正文关键词。",
        })}
      </Paragraph>

      <Title level={5}>{t("gateway.modesHelpCheckTitle", { defaultValue: "怎么确认生效" })}</Title>
      <Paragraph>
        {t("gateway.modesHelpCheck", {
          defaultValue:
            "配完看下方「最近路由」。会写「命中规划模式（依据：tools 含 ExitPlanMode）」这类依据，而不是「检测到你在做规划」。",
        })}
      </Paragraph>
      <Paragraph type="secondary">
        {t("gateway.modesHelpAgents", {
          defaultValue:
            "规划 / 改内容只有 Claude Code 与 Codex 有信号；Desktop / OpenCode / Pi / DSH / Cline 这两行不会触发。Claude Code 供应商页的「Plan / 自动」是客户端工作模式（permissions.defaultMode），不是网关「规划」行。改完后 Claude Code / Desktop / Codex 需重启 Agent 才刷新 /v1/models。",
        })}
      </Paragraph>
    </>
  );
}

function TutorialPanel() {
  const { t } = useTranslation();
  return (
    <>
      <Paragraph>
        {t("gateway.tutorialIntro", {
          defaultValue:
            "先加上游、再绑定 Agent，然后按下表抄一份。默认放日常模型（DeepSeek / Gemini 3.8 Flash），gpt-6-astra 留给思考。下拉里的名字会带上游前缀，按显示名选即可。",
        })}
      </Paragraph>

      <Title level={5}>{t("gateway.tutorialStepsTitle", { defaultValue: "第一次怎么走" })}</Title>
      <ol style={{ paddingLeft: 20, marginBottom: 20 }}>
        <li style={{ marginBottom: 8 }}>{t("gateway.tutorialStep1", { defaultValue: "「绑定应用」勾选要走网关的 Agent。Claude Code / Desktop / Codex 绑定后到供应商页把智能网关 Auto 卡设为当前。OpenCode / Pi / DSH / Cline 绑定只追加 Auto 入口、不切换。自定义 Agent 不必绑定，用服务卡上的地址和对外 API Key。" })}</li>
        <li style={{ marginBottom: 8 }}>{t("gateway.tutorialStep2", { defaultValue: "「上游池」加入供应商。模式表只能选池里的模型，不是供应商页上未入池的卡片。" })}</li>
        <li style={{ marginBottom: 8 }}>{t("gateway.tutorialStep3", { defaultValue: "至少启用「默认」并选一个模型。没选模型的行等于关掉。" })}</li>
        <li style={{ marginBottom: 8 }}>{t("gateway.tutorialStep4", { defaultValue: "Agent 里保持 auto（Claude Code / Desktop 为 claude.auto）。点了目录里的具体模型会整表跳过规则和模式。" })}</li>
        <li style={{ marginBottom: 8 }}>{t("gateway.tutorialStep5", { defaultValue: "发一条请求，看本页底部「最近路由」的模式与依据。Claude Code / Desktop / Codex 改完目录后需重启 Agent。" })}</li>
      </ol>

      <Title level={5}>{t("gateway.tutorialAgentsTitle", { defaultValue: "按 Agent 推荐配置" })}</Title>
      <Paragraph type="secondary">
        {t("gateway.tutorialAgentsLead", {
          defaultValue: "模式表是全局一份。各 Agent 接入方式和能发出的信号不同。先按下面接好，再抄示例 2 或 3。",
        })}
      </Paragraph>
      <Collapse
        size="small"
        defaultActiveKey={["claude_code"]}
        style={{ marginBottom: 20 }}
        items={TUTORIAL_AGENTS.map((id) => ({
          key: id,
          label: t(`gateway.tutorialAgent.${id}.title`, { defaultValue: id }),
          children: <AgentTutorialBody agentId={id} />,
        }))}
      />

      <Title level={5}>{t("gateway.tutorialRecipeMinimalTitle", { defaultValue: "示例 1 · 最少配置" })}</Title>
      <Paragraph type="secondary">
        {t("gateway.tutorialRecipeMinimalLead", {
          defaultValue: "只开默认，先打通。选 DeepSeek Chat 或 Gemini 3.8 Flash，不要把最强的 gpt-6-astra 放在默认。",
        })}
      </Paragraph>
      <RecipeTable rows={RECIPE_MINIMAL} />

      <Title level={5} style={{ marginTop: 20 }}>
        {t("gateway.tutorialRecipeCcrTitle", { defaultValue: "示例 2 · 经典 CCR 分流" })}
      </Title>
      <Paragraph type="secondary">
        {t("gateway.tutorialRecipeCcrLead", {
          defaultValue:
            "对应 CCR 的 default / background / think / longContext。默认和后台走日常模型，思考才上 gpt-6-astra。规划与改内容先关。",
        })}
      </Paragraph>
      <RecipeTable rows={RECIPE_CCR} />

      <Title level={5} style={{ marginTop: 20 }}>
        {t("gateway.tutorialRecipeMixTitle", { defaultValue: "示例 3 · Astra + DeepSeek + Gemini" })}
      </Title>
      <Paragraph type="secondary">
        {t("gateway.tutorialRecipeMixLead", {
          defaultValue:
            "默认 DeepSeek Chat，后台 Gemini 3.8 Flash，思考 gpt-6-astra，长文和看图走 Gemini。没有 Astra 时思考可改 DeepSeek Reasoner。没有图像模型就关掉图像生成。",
        })}
      </Paragraph>
      <RecipeTable rows={RECIPE_MIX} />

      <Title level={5} style={{ marginTop: 20 }}>
        {t("gateway.tutorialRulesTitle", { defaultValue: "示例 4 · 条件规则" })}
      </Title>
      <Paragraph>
        {t("gateway.tutorialRulesLead", {
          defaultValue:
            "规则排在显式模型之后、模式之前。Agent 保持 auto 时最有用。点了目录里已有的具体模型时，规则也不会跑。",
        })}
      </Paragraph>
      <Paragraph>
        <Text strong>{t("gateway.tutorialRulePrefixTitle", { defaultValue: "model-prefix" })}</Text>
        <span>{" — "}</span>
        {t("gateway.tutorialRulePrefixBody", {
          defaultValue: "客户端仍在要官方 id（目录里没有）时，按前缀改写到池里的模型。",
        })}
      </Paragraph>
      <Paragraph>
        <Text code>gpt-5</Text>
        {" → "}
        <Text code>gpt-6-astra</Text>
      </Paragraph>
      <Paragraph>
        <Text strong>{t("gateway.tutorialRuleConditionTitle", { defaultValue: "condition" })}</Text>
        <span>{" — "}</span>
        {t("gateway.tutorialRuleConditionBody", {
          defaultValue: "匹配栏填 JSON。左值：token_count / thinking / web_search / vision / tool / path / target_app。",
        })}
      </Paragraph>
      <Paragraph>
        <Text code>{'{"left":"target_app","operator":"==","right":"codex"}'}</Text>
      </Paragraph>
      <Paragraph type="secondary">
        {t("gateway.tutorialRuleCodexNote", {
          defaultValue: "只让 Codex 打某个更顺的上游；Claude Code 仍走模式表。",
        })}
      </Paragraph>
      <Paragraph>
        <Text code>{'{"left":"token_count","operator":">=","right":60000}'}</Text>
      </Paragraph>
      <Paragraph type="secondary">
        {t("gateway.tutorialRuleTokenNote", {
          defaultValue: "和长上下文模式同类，但规则更先命中，可用来覆盖模式表。一般二选一即可。",
        })}
      </Paragraph>
    </>
  );
}

function AgentTutorialBody({ agentId }: { agentId: TutorialAgentId }) {
  const { t } = useTranslation();
  const showClaudeCodeNote = agentId === "claude_code";
  return (
    <>
      <Paragraph>
        {t(`gateway.tutorialAgent.${agentId}.lead`, { defaultValue: "" })}
      </Paragraph>
      <ol style={{ paddingLeft: 20, marginBottom: showClaudeCodeNote ? 8 : 0 }}>
        {AGENT_STEP_KEYS.map((key) => (
          <li key={key} style={{ marginBottom: 8 }}>
            {t(`gateway.tutorialAgent.${agentId}.${key}`, { defaultValue: "" })}
          </li>
        ))}
      </ol>
      {showClaudeCodeNote ? (
        <Paragraph type="secondary">
          {t("gateway.tutorialAgent.claude_code.note", {
            defaultValue:
              "只用 claude.auto 且其它行关掉或没选模型时，最近路由只会「命中默认模式」。",
          })}
        </Paragraph>
      ) : null}
    </>
  );
}

function RecipeTable({ rows }: { rows: RecipeRow[] }) {
  const { t } = useTranslation();
  const onLabel = (on: RecipeOn) => {
    switch (on) {
      case "on":
        return t("gateway.tutorialOn", { defaultValue: "开" });
      case "off":
        return t("gateway.tutorialOff", { defaultValue: "关" });
      case "optional":
        return t("gateway.tutorialOptional", { defaultValue: "可选" });
      default: {
        const _never: never = on;
        return _never;
      }
    }
  };
  return (
    <Table
      size="small"
      pagination={false}
      rowKey={(row) => `${row.modeId}-${row.pickKey}`}
      dataSource={rows}
      columns={[
        {
          title: t("gateway.modeName", { defaultValue: "模式" }),
          dataIndex: "modeId",
          render: (id: RecipeRow["modeId"]) => t(`gateway.modes.${id}`, { defaultValue: id }),
        },
        {
          title: t("gateway.enabled", { defaultValue: "启用" }),
          dataIndex: "on",
          width: 72,
          render: (on: RecipeOn) => onLabel(on),
        },
        {
          title: t("gateway.tutorialPick", { defaultValue: "选什么（示例）" }),
          render: (_: unknown, row: RecipeRow) => t(`gateway.${row.pickKey}`),
        },
        {
          title: t("gateway.tutorialExtra", { defaultValue: "挡位 / 阈值" }),
          render: (_: unknown, row: RecipeRow) => t(`gateway.${row.extraKey}`),
        },
      ]}
    />
  );
}
