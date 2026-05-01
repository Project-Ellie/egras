import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { getToken, setToken } from "../api/client";

type Claims = {
  sub: string;
  org: string;
  exp: number;
  username?: string;
};

type AuthState = {
  token: string | null;
  claims: Claims | null;
  login: (token: string) => void;
  logout: () => void;
};

const AuthCtx = createContext<AuthState | null>(null);

function decode(token: string | null): Claims | null {
  if (!token) return null;
  try {
    const part = token.split(".")[1];
    const json = atob(part.replace(/-/g, "+").replace(/_/g, "/"));
    return JSON.parse(json) as Claims;
  } catch {
    return null;
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setTok] = useState<string | null>(getToken());
  const [claims, setClaims] = useState<Claims | null>(decode(token));

  useEffect(() => {
    setClaims(decode(token));
  }, [token]);

  const value: AuthState = {
    token,
    claims,
    login: (t) => {
      setToken(t);
      setTok(t);
    },
    logout: () => {
      setToken(null);
      setTok(null);
    },
  };
  return <AuthCtx.Provider value={value}>{children}</AuthCtx.Provider>;
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthCtx);
  if (!ctx) throw new Error("useAuth outside AuthProvider");
  return ctx;
}
