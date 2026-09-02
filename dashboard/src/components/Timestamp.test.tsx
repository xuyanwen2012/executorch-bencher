import { describe, expect, test } from "bun:test";
import { renderToString } from "react-dom/server";
import { Timestamp } from "./Timestamp";

const ISO = "2026-01-02T03:04:05.678Z";
const LOCAL = /\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}/;

describe("Timestamp", () => {
  test("an absent stamp renders the absent marker", () => {
    expect(renderToString(<Timestamp iso={null} />)).toContain("not recorded");
    expect(renderToString(<Timestamp iso={undefined} />)).toContain("not recorded");
  });

  test("shows local time with the UTC form on hover", () => {
    const html = renderToString(<Timestamp iso={ISO} />);
    expect(html).toMatch(LOCAL);
    expect(html).toContain(`${ISO} UTC`);
    expect(html).toContain("local");
  });

  test("`both` prints UTC alongside; `stacked` splits date and time", () => {
    // renderToString separates adjacent text nodes with an empty comment.
    const both = renderToString(<Timestamp iso={ISO} both />).replace(/<!-- -->/g, "");
    expect(both).toContain(`>${ISO} UTC<`);
    const stacked = renderToString(<Timestamp iso={ISO} stacked />);
    expect(stacked).toMatch(/>\d{4}-\d{2}-\d{2}</);
    expect(stacked).toMatch(/>\d{2}:\d{2}:\d{2}</);
  });

  test("`timeOnly` shows the clock alone", () => {
    const html = renderToString(<Timestamp iso={ISO} timeOnly />);
    expect(html).toMatch(/>\d{2}:\d{2}:\d{2}</);
    expect(html).not.toMatch(/>\d{4}-\d{2}-\d{2}/);
  });
});
