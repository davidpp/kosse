// Le raccord: one process stem owns two endpoint arms. All coordinates use a 16-unit grid.
export const MARK = Object.freeze({
  stemX: 1,
  stemTop: 1,
  stemBottom: 15,
  stemWidth: 4,
  armWidth: 3,
  junction: [4, 7],
  upperEnd: [10.75, 2.5],
  lowerEnd: [14, 13.5],
  notchY: 11,
  notchWidth: 2,
  notchHeight: 2,
});
export const SMALL = Object.freeze({ armWidth: 3.6 });
const r = (n) => +n.toFixed(4);
const polygon = (points) => `M${points.map((p) => p.map(r).join(" ")).join("L")}Z`;
export function markPath(options = {}, attention = false) {
  const o = { ...MARK, ...options };
  const { stemX: x, stemTop: y, stemBottom: b, stemWidth: w } = o;
  const stem = [
    [x, y],
    [x + w, y],
    [x + w, b],
    [x, b],
  ];
  if (attention)
    stem.push(
      [x, o.notchY + o.notchHeight],
      [x + o.notchWidth, o.notchY + o.notchHeight],
      [x + o.notchWidth, o.notchY],
      [x, o.notchY],
    );
  const arm = (end) => {
    const [jx, jy] = o.junction;
    const dx = end[0] - jx,
      dy = end[1] - jy;
    const scale = o.armWidth / (2 * Math.hypot(dx, dy));
    const nx = -dy * scale,
      ny = dx * scale;
    return polygon([
      [jx + nx, jy + ny],
      [jx - nx, jy - ny],
      [end[0] - nx, end[1] - ny],
      [end[0] + nx, end[1] + ny],
    ]);
  };
  return polygon(stem) + arm(o.upperEnd) + arm(o.lowerEnd);
}
export const attentionPath = (options = SMALL) => markPath(options, true);
