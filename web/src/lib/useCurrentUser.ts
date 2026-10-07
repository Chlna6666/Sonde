import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { api } from "./api";
import type { User } from "../App";

/**
 * Hook to retrieve the currently authenticated user.
 * Tries outlet context first (passed by Shell), falling back to fetching /auth/me if needed.
 */
export function useCurrentUser(): User | null {
  const context = useOutletContext<{ user?: User } | null>();
  const [user, setUser] = useState<User | null>(context?.user ?? null);

  useEffect(() => {
    if (context?.user) {
      setUser(context.user);
      return;
    }
    let active = true;
    api<User>("/api/v1/auth/me")
      .then((data) => {
        if (active && data) setUser(data);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [context?.user]);

  return user;
}
