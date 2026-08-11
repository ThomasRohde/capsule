describe("SQLite Capsule trusted native first-open shell (raw renderer intentionally absent)", () => {
  let startupFocusId = "";

  beforeAll(async () => {
    const state = await $("#host-state");
    await state.waitUntil(
      async () => (await state.getText()) !== "Verifying before open",
      { timeout: 20_000, timeoutMsg: "trusted shell did not finish its startup report" },
    );
    await browser.pause(1_000);
    const settledState = await state.getText();
    if (settledState !== "Trust decision required · code locked") {
      throw new Error(`${settledState}: ${await $("#verdict").getText()}`);
    }
    startupFocusId = await browser.execute(() => document.activeElement?.id || "");
  });

  it("keeps executable assets locked while showing verified identity", async () => {
    await $("button[data-page='trust']").click();
    await expect($("#verdict strong")).toHaveText("Your decision is required");
    await expect($("#host-state")).toHaveText("Trust decision required · code locked");
    expect(await browser.execute(() => document.querySelector("#boundary-title")?.textContent)).toBe("Application window · executable assets locked");
    await expect($("#identity-details")).toHaveText(expect.stringContaining("Diagram Studio — SQLite Capsule"));
    await expect($("#identity-details")).toHaveText(expect.stringContaining("org.sqlite-capsule.diagram-studio 0.3.0"));
    await expect($("#identity-details")).toHaveText(expect.stringContaining("Executable assets\nNot released"));
  });

  it("renders the host-owned capability decision without creating authority", async () => {
    await $("button[data-page='capabilities']").click();
    const capabilities = await $$("#capability-list input[type='checkbox']");
    expect(capabilities.length).toBe(7);
    await expect($("button[data-action='allow_once']")).toBeEnabled();
    await expect($("#always-button")).toBeDisabled();
    await expect($("button[data-action='deny']")).toBeEnabled();
    await expect($("#forget-decision-button")).toBeDisabled();
    await expect($("#action-status")).toHaveText("");
  });

  it("places real WebView2 keyboard focus on the first-open heading", async () => {
    expect(startupFocusId).toBe("prompt-title");
    await expect($("#prompt-title")).toHaveText("Choose what this release may do");
  });

  it("traverses every enabled prompt control in DOM order without a keyboard trap", async () => {
    await $("button[data-page='capabilities']").click();
    const focusable = await browser.execute(() => [
      ...document.querySelectorAll("#capability-list input:not(:disabled)"),
      ...document.querySelectorAll("#actions button:not(:disabled)"),
    ].map((element) => {
      if (element instanceof HTMLInputElement) return `input:${element.value}`;
      return `button:${element.dataset.action}`;
    }));
    expect(focusable.length).toBe(10);

    const visited = [];
    for (const expected of focusable) {
      await browser.keys(["Tab"]);
      const active = await browser.execute(() => {
        const element = document.activeElement;
        const key = element instanceof HTMLInputElement
          ? `input:${element.value}`
          : `button:${element?.dataset?.action || ""}`;
        return {
          key,
          focusVisible: element?.matches(":focus-visible") || false,
        };
      });
      expect(active.key).toBe(expected);
      expect(active.focusVisible).toBe(true);
      visited.push(active.key);
    }
    expect(visited).toEqual(focusable);

    await browser.keys(["Shift", "Tab"]);
    const reverseActive = await browser.execute(() => document.activeElement?.dataset?.action || "");
    expect(reverseActive).toBe("deny");
  });

  it("denies and forgets only the isolated exact-file decision without granting authority", async () => {
    await $("button[data-page='capabilities']").click();
    await $("button[data-action='deny']").click();
    await browser.waitUntil(
      async () => browser.execute(() => document.querySelector("#verdict strong")?.textContent === "Execution is blocked"),
      { timeout: 20_000, timeoutMsg: "the host did not apply the exact-file denial" },
    );
    expect(await browser.execute(() => document.querySelector("#boundary-title")?.textContent)).toBe("Application window · executable assets locked");
    await expect($("#action-status")).toHaveText("Decision applied. The effective policy is shown above.");

    await $("button[data-page='admin']").click();
    await expect($("#forget-decision-button")).toBeEnabled();
    await $("#forget-decision-button").scrollIntoView();
    await $("#forget-decision-button").click();
    await browser.waitUntil(
      async () => {
        try {
          return (await browser.getAlertText()).includes("FORGET-CURRENT-DECISION");
        } catch {
          return false;
        }
      },
      { timeout: 10_000, timeoutMsg: "the exact-decision confirmation prompt did not open" },
    );
    await browser.sendAlertText("FORGET-CURRENT-DECISION");
    await browser.acceptAlert();
    await browser.waitUntil(
      async () => browser.execute(() => document.querySelector("#verdict strong")?.textContent === "Your decision is required"),
      { timeout: 20_000, timeoutMsg: "forgetting the decision did not restore first-open policy" },
    );
    await $("button[data-page='admin']").click();
    expect(await browser.execute(() => document.querySelector("#boundary-title")?.textContent)).toBe("Application window · executable assets locked");
    await expect($("#forget-decision-button")).toBeDisabled();
    await expect($("#admin-output")).toHaveText(expect.stringContaining('"authority_granted": false'));
    await $("button[data-page='capabilities']").click();
    await expect($("button[data-action='allow_once']")).toBeEnabled();
  });
});
