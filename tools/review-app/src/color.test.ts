import { describe, expect, it } from "vite-plus/test";
import { describeColor, hueName, lightness, rgbToHsl, toHex } from "./color";

describe("toHex", () => {
  it("is the upper-case six-digit spelling other tools expect", () => {
    expect(toHex({ r: 0, g: 0, b: 0 })).toBe("#000000");
    expect(toHex({ r: 232, g: 230, b: 225 })).toBe("#E8E6E1");
    expect(toHex({ r: 255, g: 255, b: 255 })).toBe("#FFFFFF");
  });

  it("clamps and rounds rather than emitting a malformed code", () => {
    expect(toHex({ r: -5, g: 300, b: 127.6 })).toBe("#00FF80");
  });
});

describe("rgbToHsl", () => {
  it("reports no hue for a grey", () => {
    expect(rgbToHsl({ r: 128, g: 128, b: 128 })).toMatchObject({ h: 0, s: 0 });
  });

  it("places the primaries on the wheel", () => {
    expect(rgbToHsl({ r: 255, g: 0, b: 0 }).h).toBeCloseTo(0);
    expect(rgbToHsl({ r: 0, g: 255, b: 0 }).h).toBeCloseTo(120);
    expect(rgbToHsl({ r: 0, g: 0, b: 255 }).h).toBeCloseTo(240);
  });
});

describe("lightness", () => {
  it("runs 0 to 100 over the greyscale", () => {
    expect(lightness({ r: 0, g: 0, b: 0 })).toBeCloseTo(0);
    expect(lightness({ r: 255, g: 255, b: 255 })).toBeCloseTo(100);
    // Mid-grey in sRGB is about L* 53, not 50 — which is the reason this is
    // not HSL's `l`.
    expect(lightness({ r: 128, g: 128, b: 128 })).toBeCloseTo(53.6, 1);
  });

  // The failure HSL's `l` has, stated as a test: it calls these two the same.
  it("separates yellow from blue, which HSL lightness does not", () => {
    expect(rgbToHsl({ r: 255, g: 255, b: 0 }).l).toBeCloseTo(0.5);
    expect(rgbToHsl({ r: 0, g: 0, b: 255 }).l).toBeCloseTo(0.5);
    expect(lightness({ r: 255, g: 255, b: 0 })).toBeGreaterThan(90);
    expect(lightness({ r: 0, g: 0, b: 255 })).toBeLessThan(40);
  });
});

describe("hueName", () => {
  it("names each sector, wrapping red across zero", () => {
    expect(hueName(0)).toBe("red");
    expect(hueName(355)).toBe("red");
    expect(hueName(-5)).toBe("red");
    expect(hueName(30)).toBe("orange");
    expect(hueName(120)).toBe("green");
    expect(hueName(180)).toBe("cyan");
    expect(hueName(240)).toBe("blue");
    expect(hueName(300)).toBe("purple");
  });
});

describe("describeColor", () => {
  it("names the neutrals on the grey scale", () => {
    expect(describeColor({ r: 255, g: 255, b: 255 })).toBe("white");
    expect(describeColor({ r: 0, g: 0, b: 0 })).toBe("black");
    expect(describeColor({ r: 128, g: 128, b: 128 })).toBe("grey");
  });

  // A direction invented from a span of two code values is noise, and inventing
  // one on a page about colour casts is worse than saying nothing.
  it("calls a colour this close to neutral grey, with no hue at all", () => {
    expect(describeColor({ r: 130, g: 132, b: 133 })).toBe("grey");
  });

  // The whole reason the readout exists: a faint cast is what an eye cannot
  // name. "grey" throws the finding away; "muted green" overstates a colour
  // anyone would call grey.
  it("names a near-neutral on the grey scale and says which way it leans", () => {
    expect(describeColor({ r: 120, g: 140, b: 120 })).toBe("grey, slightly green");
    expect(describeColor({ r: 232, g: 230, b: 225 })).toBe("near-white, slightly yellow");
  });

  // The reason the band is measured in channel span rather than in HSL
  // saturation: these two are the same kind of finding — a grey leaning cool —
  // but `s` reports them as 4% and 10%, so no single cutoff in `s` catches both
  // without also naming one-code-value noise on a near-white.
  it("catches a faint cast at any lightness, not only at mid grey", () => {
    expect(describeColor({ r: 186, g: 188, b: 191 })).toBe("light grey, slightly blue");
    expect(describeColor({ r: 86, g: 96, b: 106 })).toBe("dark grey, slightly sky blue");
  });

  // The lightness word and the L* the chip prints beside it come from the same
  // number, so a chip can never say "light" next to an L* of 30.
  it("takes its lightness word from L*, not from HSL lightness", () => {
    // `l` 0.65 but L* 68.5 — the two sit on opposite sides of "light grey".
    expect(describeColor({ r: 167, g: 167, b: 167 })).toBe("light grey");
    expect(lightness({ r: 167, g: 167, b: 167 })).toBeGreaterThan(66);
  });

  it("qualifies a real hue by lightness and by low saturation", () => {
    expect(describeColor({ r: 150, g: 200, b: 255 })).toBe("light sky blue");
    expect(describeColor({ r: 0, g: 60, b: 20 })).toBe("dark green");
    expect(describeColor({ r: 0, g: 26, b: 8 })).toBe("very dark green");
    expect(describeColor({ r: 90, g: 130, b: 90 })).toBe("muted green");
  });
});
