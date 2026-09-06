/* Table: skin only. Header uses fine-size uppercase letter-spacing, thin lines
   between rows, hovered row becomes surface-2. Column alignment and width are
   decided by the page; this does no sorting and no pagination (Pager is in index.tsx). */
import type {
  HTMLAttributes,
  TableHTMLAttributes,
  TdHTMLAttributes,
  ThHTMLAttributes,
} from "react";
import { cn } from "./index";

export function Table({
  className,
  ...props
}: TableHTMLAttributes<HTMLTableElement>) {
  return (
    <div className="w-full overflow-x-auto">
      <table
        className={cn("w-full border-collapse text-body text-ink", className)}
        {...props}
      />
    </div>
  );
}

export function THead({
  className,
  ...props
}: HTMLAttributes<HTMLTableSectionElement>) {
  return <thead className={cn("text-left", className)} {...props} />;
}

export function TBody({
  className,
  ...props
}: HTMLAttributes<HTMLTableSectionElement>) {
  return <tbody className={className} {...props} />;
}

export function Tr({
  interactive,
  className,
  ...props
}: HTMLAttributes<HTMLTableRowElement> & {
  /** Clickable row: hover fills the row, cursor becomes a pointer */
  interactive?: boolean;
}) {
  return (
    <tr
      className={cn(
        "border-b border-line",
        interactive && "cursor-pointer transition-colors duration-fast hover:bg-surface-2",
        className,
      )}
      {...props}
    />
  );
}

export function Th({
  className,
  ...props
}: ThHTMLAttributes<HTMLTableCellElement>) {
  return (
    <th
      className={cn(
        "border-b border-line-strong px-3 py-2 text-fine font-medium uppercase tracking-wider text-ink-3",
        className,
      )}
      {...props}
    />
  );
}

export function Td({
  className,
  ...props
}: TdHTMLAttributes<HTMLTableCellElement>) {
  return <td className={cn("px-3 py-2 align-top", className)} {...props} />;
}
