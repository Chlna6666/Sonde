// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import {
  isUserOwnedApp,
  getUserPreferredAppId,
  setUserPreferredAppId,
  resolveInitialAppId,
  sortAppsForUser,
  formatAppOptions,
  type BaseApplication,
} from "./applicationPreferences";
import type { User } from "../App";

describe("applicationPreferences", () => {
  const userA: User = {
    id: "user-a",
    email: "a@test.com",
    username: "AdminA",
    locale: "zh-CN",
    roles: ["Admin"],
    csrfToken: "token",
  };

  const userB: User = {
    id: "user-b",
    email: "b@test.com",
    username: "AdminB",
    locale: "zh-CN",
    roles: ["Admin"],
    csrfToken: "token",
  };

  const apps: BaseApplication[] = [
    { id: "app-shared", name: "Shared App", ownerUserId: null },
    { id: "app-a", name: "App of Admin A", ownerUserId: "user-a" },
    { id: "app-b", name: "App of Admin B", ownerUserId: "user-b" },
    { id: "app-a2", name: "Second App of Admin A", ownerUserId: "user-a" },
  ];

  beforeEach(() => {
    localStorage.clear();
  });

  describe("isUserOwnedApp", () => {
    it("identifies ownership correctly", () => {
      expect(isUserOwnedApp(apps[1], userA)).toBe(true);
      expect(isUserOwnedApp(apps[1], userB)).toBe(false);
      expect(isUserOwnedApp(apps[0], userA)).toBe(false);
      expect(isUserOwnedApp(apps[1], null)).toBe(false);
    });
  });

  describe("resolveInitialAppId", () => {
    it("defaults to user's first owned application when no preference is saved", () => {
      const resolvedForA = resolveInitialAppId(apps, userA);
      expect(resolvedForA).toBe("app-a");

      const resolvedForB = resolveInitialAppId(apps, userB);
      expect(resolvedForB).toBe("app-b");
    });

    it("falls back to allowAll when user owns no apps and allowAll is true", () => {
      const userC: User = { ...userA, id: "user-c" };
      const resolved = resolveInitialAppId(apps, userC, { allowAll: true, allValue: "all" });
      expect(resolved).toBe("all");
    });

    it("falls back to apps[0] when user owns no apps and allowAll is false", () => {
      const userC: User = { ...userA, id: "user-c" };
      const resolved = resolveInitialAppId(apps, userC);
      expect(resolved).toBe("app-shared");
    });

    it("respects explicitly passed query param if valid", () => {
      const resolved = resolveInitialAppId(apps, userA, { explicitId: "app-b" });
      expect(resolved).toBe("app-b");
    });

    it("prioritizes page-specific saved preference", () => {
      setUserPreferredAppId(userA, "app-a2", "sonde_explorer_app");
      const resolved = resolveInitialAppId(apps, userA, { pagePrefix: "sonde_explorer_app" });
      expect(resolved).toBe("app-a2");
    });

    it("prioritizes global preference if no page-specific preference is saved", () => {
      setUserPreferredAppId(userA, "app-a2", "sonde_preferred_app");
      const resolved = resolveInitialAppId(apps, userA);
      expect(resolved).toBe("app-a2");
    });
  });

  describe("sortAppsForUser", () => {
    it("places user-owned applications first", () => {
      const sortedForB = sortAppsForUser(apps, userB);
      expect(sortedForB[0].id).toBe("app-b");
    });
  });

  describe("formatAppOptions", () => {
    it("formats options with '(我的应用)' for owned apps", () => {
      const options = formatAppOptions(apps, userA, { myAppLabel: "我的应用" });
      const optA = options.find((o) => o.value === "app-a");
      const optB = options.find((o) => o.value === "app-b");
      expect(optA?.label).toBe("App of Admin A (我的应用)");
      expect(optB?.label).toBe("App of Admin B");
    });

    it("supports prepending all option", () => {
      const options = formatAppOptions(apps, userA, {
        includeAll: true,
        allValue: "all",
        allLabel: "全部应用",
      });
      expect(options[0]).toEqual({ value: "all", label: "全部应用" });
    });
  });
});
