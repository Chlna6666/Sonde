import { describe, it, expect } from "vitest";
import i18n, { initI18n } from "./i18n";
import enJson from "./locales/en.json";
import zhJson from "./locales/zh-CN.json";

describe("i18n dynamic loading & JSON locales", () => {
  it("should have identical key sets between zh-CN and en JSON files", () => {
    const enKeys = Object.keys(enJson).sort();
    const zhKeys = Object.keys(zhJson).sort();

    expect(enKeys.length).toBe(zhKeys.length);
    expect(enKeys).toEqual(zhKeys);
  });

  it("should initialize and resolve translations dynamically", async () => {
    await initI18n();
    expect(i18n.t("brand.name")).toBe("Sonde");

    await i18n.changeLanguage("en");
    expect(i18n.t("overview.title")).toBe("Telemetry Overview");
    expect(i18n.t("common.save")).toBe("Save Changes");

    await i18n.changeLanguage("zh-CN");
    expect(i18n.t("overview.title")).toBe("遥测数据总览");
    expect(i18n.t("common.save")).toBe("保存修改");
  });
});
