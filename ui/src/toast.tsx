import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { AlertIcon, CheckIcon } from "./components/icons";
import { cx } from "./lib/format";
import { listen } from "./lib/tauri";
import type { Toast as ToastPayload } from "./lib/tauri";
import "./index.css";

function Toast() {
  // A számláló új kulcsot ad minden értesítésnek, így az animáció egymást követő értesítéseknél is újraindul
  const [toast, setToast] = useState<(ToastPayload & { id: number }) | null>(null);

  useEffect(() => {
    const unlisten = listen<ToastPayload>("toast", ({ payload }) => setToast((prev) => ({ ...payload, id: (prev?.id ?? 0) + 1 })));
    return () => { unlisten.then((off) => off()); };
  }, []);

  if (!toast) return null;
  const error = toast.kind === "error";
  return (
    <div
      key={toast.id}
      className="absolute inset-1.5 flex animate-toast items-center gap-3.5 rounded-xl border border-white/8 bg-[rgba(24,25,29,.94)] pr-[18px] pl-4 text-[#e8e9ec] opacity-0"
    >
      <div
        className={cx(
          "grid size-[38px] flex-none place-items-center rounded-full [&_svg]:size-[22px]",
          error ? "bg-[rgba(239,68,68,.16)] text-[#f87171]" : "bg-[rgba(74,222,128,.16)] text-[#4ade80]",
        )}
      >
        {error ? <AlertIcon /> : <CheckIcon />}
      </div>
      <div className="min-w-0 leading-[1.35]">
        <div className="text-[15px] font-semibold">{toast.title}</div>
        <div className="truncate text-[12.5px] text-[#a1a5ae]">{toast.detail}</div>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<Toast />);
