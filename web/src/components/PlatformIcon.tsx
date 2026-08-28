import type { FC } from "react";
import {
  FaWindows,
  FaApple,
  FaLinux,
  FaAndroid,
  FaMobileScreen,
  FaUbuntu,
  FaFedora,
  FaDebian,
  FaLaptop,
} from "react-icons/fa6";
import {
  SiArchlinux,
  SiNixos,
  SiLinuxmint,
} from "react-icons/si";

export type SpecificPlatform =
  | "windows"
  | "linux"
  | "macos"
  | "android"
  | "ios"
  | "ubuntu"
  | "fedora"
  | "debian"
  | "arch"
  | "nixos"
  | "mint"
  | "unknown";

export type PlatformFamily = "windows" | "linux" | "macos" | "mobile" | "unknown";

export function detectPlatform(name: string): SpecificPlatform {
  const lower = name.toLowerCase();
  if (lower.includes("ubuntu")) return "ubuntu";
  if (lower.includes("fedora")) return "fedora";
  if (lower.includes("debian")) return "debian";
  if (lower.includes("arch")) return "arch";
  if (lower.includes("nixos")) return "nixos";
  if (lower.includes("mint")) return "mint";
  if (lower.startsWith("win") || lower.includes("windows")) return "windows";
  if (lower.includes("linux")) return "linux";
  if (lower.includes("mac") || lower.includes("darwin") || lower.includes("osx") || lower.includes("apple")) {
    return "macos";
  }
  if (lower.includes("android")) return "android";
  if (lower.includes("ios") || lower.includes("iphone") || lower.includes("ipad")) return "ios";
  return "unknown";
}

export function getPlatformFamily(name: string): PlatformFamily {
  const lower = name.toLowerCase();
  if (
    lower.includes("linux") ||
    lower.includes("fedora") ||
    lower.includes("nixos") ||
    lower.includes("mint") ||
    lower.includes("ubuntu") ||
    lower.includes("debian") ||
    lower.includes("arch") ||
    lower.includes("centos") ||
    lower.includes("rhel") ||
    lower.includes("manjaro")
  ) {
    return "linux";
  }
  if (lower.startsWith("win") || lower.includes("windows")) return "windows";
  if (lower.includes("mac") || lower.includes("darwin") || lower.includes("osx") || lower.includes("apple")) {
    return "macos";
  }
  if (lower.includes("android") || lower.includes("ios") || lower.includes("iphone") || lower.includes("ipad")) {
    return "mobile";
  }
  return "unknown";
}

export function getPlatformColor(name: string): string {
  const specific = detectPlatform(name);
  switch (specific) {
    case "windows":
      return "#38bdf8";
    case "fedora":
      return "#51a2da";
    case "nixos":
      return "#5277c3";
    case "mint":
      return "#87cf3e";
    case "ubuntu":
      return "#e95420";
    case "debian":
      return "#d70a53";
    case "arch":
      return "#1793d1";
    case "linux":
      return "#22c55e";
    case "macos":
      return "#a855f7";
    case "android":
      return "#10b981";
    case "ios":
      return "#f43f5e";
    default:
      return "#f59e0b";
  }
}

interface PlatformIconProps {
  platform: SpecificPlatform | PlatformFamily | string;
  size?: number;
  className?: string;
}

export const PlatformIcon: FC<PlatformIconProps> = ({
  platform,
  size = 14,
  className = "",
}) => {
  const type = typeof platform === "string" ? detectPlatform(platform) : platform;

  switch (type) {
    case "windows":
      return <FaWindows size={size} className={className} aria-hidden="true" />;
    case "ubuntu":
      return <FaUbuntu size={size} className={className} aria-hidden="true" />;
    case "fedora":
      return <FaFedora size={size} className={className} aria-hidden="true" />;
    case "debian":
      return <FaDebian size={size} className={className} aria-hidden="true" />;
    case "arch":
      return <SiArchlinux size={size} className={className} aria-hidden="true" />;
    case "nixos":
      return <SiNixos size={size} className={className} aria-hidden="true" />;
    case "mint":
      return <SiLinuxmint size={size} className={className} aria-hidden="true" />;
    case "linux":
      return <FaLinux size={size} className={className} aria-hidden="true" />;
    case "macos":
      return <FaApple size={size} className={className} aria-hidden="true" />;
    case "android":
      return <FaAndroid size={size} className={className} aria-hidden="true" />;
    case "ios":
      return <FaMobileScreen size={size} className={className} aria-hidden="true" />;
    default:
      return <FaLaptop size={size} className={className} aria-hidden="true" />;
  }
};
