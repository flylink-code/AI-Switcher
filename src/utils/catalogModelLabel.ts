const DISPLAY_SEP = " · ";

export type CatalogModelLabelSource = {
  publicId: string;
  displayName?: string | null;
  providerName?: string | null;
};

export type CatalogModelView = {
  short: string;
  provider: string;
  title: string;
  searchText: string;
};

const MODEL_FAMILY_PREFIX =
  /^(gpt|claude|gemini|kimi|deepseek|o[0-9]|glm|qwen|doubao|moonshot|llama|mistral|grok|sonnet|opus|haiku|fable|astra)/i;

function providerSlug(name: string): string {
  const slug = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug;
}

function stripKnownProviderPrefix(publicId: string, providerName?: string): string {
  let rest = publicId;
  const slug = providerName ? providerSlug(providerName) : "";
  if (slug) {
    const claudePrefixed = `claude.${slug}.`;
    if (rest.toLowerCase().startsWith(claudePrefixed)) {
      return rest.slice(claudePrefixed.length);
    }
    if (rest.toLowerCase().startsWith(`${slug}.`)) {
      return rest.slice(slug.length + 1);
    }
  }
  if (/^claude\./i.test(rest) && rest.includes(".", 7)) {
    rest = rest.replace(/^claude\./i, "");
  }
  const dot = rest.indexOf(".");
  if (dot < 0) {
    return rest;
  }
  const first = rest.slice(0, dot);
  if (MODEL_FAMILY_PREFIX.test(first)) {
    return rest;
  }
  return rest.slice(dot + 1);
}

function slugFromDisplayName(displayName: string): string {
  const index = displayName.indexOf(DISPLAY_SEP);
  if (index < 0) {
    return "";
  }
  return displayName.slice(index + DISPLAY_SEP.length).trim();
}

function providerFromDisplayName(displayName: string): string {
  const index = displayName.indexOf(DISPLAY_SEP);
  if (index < 0) {
    return "";
  }
  return displayName.slice(0, index).trim();
}

/** Short UI label for a catalog publicId. Stored values stay the full id. */
export function catalogModelView(source: CatalogModelLabelSource): CatalogModelView {
  const publicId = String(source.publicId || "").trim();
  if (!publicId || publicId.toLowerCase() === "auto") {
    return {
      short: publicId || "auto",
      provider: "",
      title: "auto",
      searchText: "auto",
    };
  }
  const displayName = String(source.displayName || "").trim();
  const provider =
    String(source.providerName || "").trim() || providerFromDisplayName(displayName);
  const short =
    slugFromDisplayName(displayName) || stripKnownProviderPrefix(publicId, provider) || publicId;
  const title =
    provider && provider.toLowerCase() !== short.toLowerCase()
      ? `${provider}${DISPLAY_SEP}${short}`
      : displayName || publicId;
  const searchText = [short, provider, publicId, displayName]
    .filter((part, index, all) => part && all.indexOf(part) === index)
    .join(" ");
  return { short, provider, title, searchText };
}

export function catalogModelShortLabel(
  publicId: string | null | undefined,
  catalog?: CatalogModelLabelSource[],
): string {
  const id = String(publicId || "").trim();
  if (!id) {
    return "";
  }
  const entry = catalog?.find((item) => item.publicId === id);
  return catalogModelView(entry ?? { publicId: id }).short;
}

