import { describe, expect, it } from "vitest";

import { MOBILE_GUIDE } from "./mobileGuide";

describe("mobile guide content", () => {
  it("covers the required sections", () => {
    const titles = MOBILE_GUIDE.map((section) => section.title);
    expect(titles).toContain("页面说明");
    expect(titles).toContain("配对与同步");
    expect(titles).toContain("提醒说明");
    expect(titles).toContain("只读限制");
  });

  it("states the read-only limits explicitly", () => {
    const section = MOBILE_GUIDE.find((item) => item.title === "只读限制");
    const text = (section?.items ?? []).join("");
    expect(text).toContain("创建");
    expect(text).toContain("编辑");
    expect(text).toContain("删除");
    expect(text).toContain("完成");
  });

  it("explains the exact alarm permission", () => {
    const text = JSON.stringify(MOBILE_GUIDE);
    expect(text).toContain("精确闹钟");
    expect(text).toContain("重启");
  });
});
