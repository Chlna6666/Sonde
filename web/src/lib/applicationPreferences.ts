import type { User } from "../App";

export type BaseApplication = {
  id: string;
  name: string;
  ownerUserId?: string | null;
  createdAt?: number;
  [key: string]: unknown;
};

/**
 * Returns true if the application is owned by the current user.
 */
export function isUserOwnedApp(app: BaseApplication, user?: User | null): boolean {
  if (!user?.id || !app?.ownerUserId) return false;
  return String(app.ownerUserId).trim().toLowerCase() === String(user.id).trim().toLowerCase();
}

/**
 * Gets the localStorage key for user preference.
 */
export function getUserAppPreferenceKey(userId: string, prefix = "sonde_preferred_app"): string {
  return `${prefix}_${userId}`;
}

/**
 * Retrieves the user's saved application preference from localStorage.
 */
export function getUserPreferredAppId(user?: User | null, prefix = "sonde_preferred_app"): string | null {
  if (!user?.id) return null;
  try {
    return localStorage.getItem(getUserAppPreferenceKey(user.id, prefix));
  } catch {
    return null;
  }
}

/**
 * Saves the user's application preference to localStorage.
 */
export function setUserPreferredAppId(
  user: User | null | undefined,
  appId: string,
  prefix = "sonde_preferred_app"
): void {
  if (!user?.id) return;
  try {
    localStorage.setItem(getUserAppPreferenceKey(user.id, prefix), appId);
  } catch {
    // Ignore storage quota or security errors
  }
}

/**
 * Sets a newly created or imported application ID as preferred across all Sonde pages.
 */
export function setCreatedAppAsPreferred(
  user: User | null | undefined,
  appId: string
): void {
  if (!user?.id || !appId) return;
  const prefixes = [
    "sonde_preferred_app",
    "sonde_overview_app",
    "sonde_devices_app",
    "sonde_explorer_app",
    "sonde_alerts_app",
    "sonde_migration_app",
  ];
  for (const prefix of prefixes) {
    setUserPreferredAppId(user, appId, prefix);
  }
}

export type ResolveAppOptions = {
  allowAll?: boolean;
  allValue?: string;
  pagePrefix?: string;
  explicitId?: string | null;
};

/**
 * Resolves the initial application ID for a user:
 * 1. Explicit ID (e.g. from URL search param) if valid in apps.
 * 2. If user owns application(s):
 *    a. Saved page preference if it is one of user's OWNED applications.
 *    b. Saved global preference if it is one of user's OWNED applications.
 *    c. If allowAll is true and page preference is explicitly saved as allValue, return allValue.
 *    d. Default to user's newest owned application (ownedApps[0].id).
 *    (Stale preferences for applications owned by OTHER users are ignored in favor of the user's owned app).
 * 3. If user owns NO applications:
 *    a. Saved page-specific preference in localStorage (if valid).
 *    b. Saved global user preference in localStorage (if valid).
 *    c. If allowAll is true, returns allValue.
 *    d. Fallback to apps[0]?.id.
 */
export function resolveInitialAppId<T extends BaseApplication>(
  apps: T[],
  user?: User | null,
  options?: ResolveAppOptions
): string {
  const allVal = options?.allValue ?? "";
  if (apps.length === 0) {
    return options?.allowAll ? allVal : "";
  }

  // 1. Explicit ID (e.g., from URL query parameter)
  if (options?.explicitId) {
    if (options.allowAll && options.explicitId === allVal) {
      return options.explicitId;
    }
    if (apps.some((a) => a.id === options.explicitId)) {
      return options.explicitId;
    }
  }

  if (user?.id) {
    const ownedApps = sortAppsForUser(
      apps.filter((a) => isUserOwnedApp(a, user)),
      user
    );

    const pageSaved = options?.pagePrefix ? getUserPreferredAppId(user, options.pagePrefix) : null;
    const globalSaved = getUserPreferredAppId(user, "sonde_preferred_app");

    // If user has owned applications, prioritize them over other users' apps
    if (ownedApps.length > 0) {
      // 2a. If page saved preference points to one of the user's owned apps
      if (pageSaved && ownedApps.some((a) => a.id === pageSaved)) {
        return pageSaved;
      }

      // 2b. If global saved preference points to one of the user's owned apps
      if (globalSaved && ownedApps.some((a) => a.id === globalSaved)) {
        return globalSaved;
      }

      // 2c. If page saved preference was explicitly allValue and allowAll is enabled
      if (options?.allowAll && pageSaved === allVal) {
        return allVal;
      }

      // 2d. Default to user's newest owned application
      return ownedApps[0].id;
    }

    // User owns NO applications: fallback to saved preferences
    if (pageSaved) {
      if (options?.allowAll && pageSaved === allVal) {
        return pageSaved;
      }
      if (apps.some((a) => a.id === pageSaved)) {
        return pageSaved;
      }
    }

    if (globalSaved && apps.some((a) => a.id === globalSaved)) {
      return globalSaved;
    }
  }

  // 3. Default to all if allowAll is enabled
  if (options?.allowAll) {
    return allVal;
  }

  // 4. Fallback to the first available application
  return apps[0].id;
}

/**
 * Sorts applications putting the user's owned applications first, and within the same tier by newest createdAt descending.
 */
export function sortAppsForUser<T extends BaseApplication>(apps: T[], user?: User | null): T[] {
  if (!user?.id) return [...apps];
  return [...apps].sort((a, b) => {
    const aOwned = isUserOwnedApp(a, user) ? 1 : 0;
    const bOwned = isUserOwnedApp(b, user) ? 1 : 0;
    if (aOwned !== bOwned) {
      return bOwned - aOwned;
    }
    const aCreated = typeof a.createdAt === "number" ? a.createdAt : 0;
    const bCreated = typeof b.createdAt === "number" ? b.createdAt : 0;
    if (aCreated !== bCreated) {
      return bCreated - aCreated;
    }
    return a.name.localeCompare(b.name);
  });
}

export type FormatAppOptionsParams = {
  myAppLabel?: string;
  sortOwnedFirst?: boolean;
  includeAll?: boolean;
  allLabel?: string;
  allValue?: string;
};

/**
 * Formats options array for CustomSelect, marking owned applications with "(我的应用)".
 */
export function formatAppOptions<T extends BaseApplication>(
  apps: T[],
  user?: User | null,
  options?: FormatAppOptionsParams
): Array<{ value: string; label: string }> {
  const sorted = options?.sortOwnedFirst !== false ? sortAppsForUser(apps, user) : apps;
  const myLabel = options?.myAppLabel ?? "我的应用";

  const appOptions = sorted.map((app) => {
    const isOwned = isUserOwnedApp(app, user);
    return {
      value: app.id,
      label: isOwned ? `${app.name} (${myLabel})` : app.name,
    };
  });

  if (options?.includeAll) {
    return [
      {
        value: options.allValue ?? "",
        label: options.allLabel ?? "全部应用",
      },
      ...appOptions,
    ];
  }

  return appOptions;
}
