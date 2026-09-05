import type { SelectHTMLAttributes } from "react";
import { ChevronDown } from "lucide-react";

export function DetailSelect({ children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <div className="select-wrap detail-select"><select {...props}>{children}</select><ChevronDown size={16} /></div>;
}
