import { expect, it } from "vitest";
import hu from "../../locales/hu.json";
import en from "../../locales/en-US.json";
import { i18n, t } from "./i18n";

it("translates every message and preserves every interpolation parameter", () => {
  expect(Object.keys(en).sort()).toEqual(Object.keys(hu).sort());
  const parameters = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map(m => m[1]).sort();
  for (const key of Object.keys(hu) as (keyof typeof hu)[]) {
    expect(en[key].trim(), key).not.toBe("");
    expect(parameters(en[key]), key).toEqual(parameters(hu[key]));
  }
});

it("treats user values as plain interpolation rather than replacement syntax", () => {
  i18n.messages = en;
  expect(t("update.available", { version: "$&" })).toContain("$&");
});
