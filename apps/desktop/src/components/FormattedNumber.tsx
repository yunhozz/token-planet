import { Fragment } from "react";

export function FormattedNumber({ value, maximumFractionDigits }: { value: number; maximumFractionDigits?: number }) {
  const formatted = value.toLocaleString("ko-KR", maximumFractionDigits === undefined ? undefined : { maximumFractionDigits });
  return <>{formatted.split(",").map((group, index) => <Fragment key={index}>{index > 0 && <>,<wbr /></>}{group}</Fragment>)}</>;
}
