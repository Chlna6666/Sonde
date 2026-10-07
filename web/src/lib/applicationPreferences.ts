import type { User } from "../App";

export type BaseApplication = {
  id: string;
  name: string;
  ownerUserId?: string | null;
  [key: string]: unknown;
};

/**
 * Returns true if the application is owned by the current user.
 */
export function isUserOwnedApp(app: BaseApplication, user?: User | null): boolean {
  if (!user?.id || !app.ownerUserId) return false;
  return app.ownerUserId === user.id;
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

export type ResolveAppOptions = {
  allowAll?: boolean;
  allValue?: string;
  pagePrefix?: string;
  explicitId?: string | null;
};

/**
 * Resolves the initial application ID for a user:
 * 1. Explicit ID (e.g. from URL search param) if valid in apps.
 * 2. Saved page-specific preference in localStorage (if provided & valid).
 * 3. Saved global user preference in localStorage (if valid).
 * 4. User's owned application (first owned app where ownerUserId === user.id).
 * 5. If allowAll is true, returns allValue (default "").
 * 6. Fallback to apps[0]?.id.
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
    // 2. Saved page-specific preference
    if (options?.pagePrefix) {
      const pageSaved = getUserPreferredAppId(user, options.pagePrefix);
      if (pageSaved) {
        if (options.allowAll && pageSaved === allVal) {
          return pageSaved;
        }
        if (apps.some((a) => a.id === pageSaved)) {
          return pageSaved;
        }
      }
    }

    // 3. Saved global preference
    const globalSaved = getUserPreferredAppId(user, "sonde_preferred_app");
    if (globalSaved && apps.some((a) => a.id === globalSaved)) {
      return globalSaved;
    }

    // 4. User's first owned application
    const ownedApps = apps.filter((a) => a.ownerUserId === user.id);
    if (ownedApps.length > 0) {
      return ownedApps[0].id;
    }
  }

  // 5. Default to all if allowAll is enabled
  if (options?.allowAll) {
    return allVal;
  }

  // 6. Fallback to the first available application
  return apps[0].id;
}

/**
 * Sorts applications putting the user's owned applications first.
 */
export function sortAppsForUser<T extends BaseApplication>(apps: T[], user?: User | null): T[] {
  if (!user?.id) return [...apps];
  return [...apps].sort((a, b) => {
    const aOwned = a.ownerUserId === user.id ? 1 : 0;
    const bOwned = b.ownerUserId === user.id ? 1 : 0;
    if (aOwned !== bOwned) {
      return bOwned - aOwned;
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
