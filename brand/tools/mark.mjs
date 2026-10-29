// Original Relay geometry. The tray has its own optical master, not a scaled app mark.
export function relay(colour = "currentColor", preview = false) {
  const stem = preview
    ? '<rect x="15" y="14" width="14" height="31" rx="6"/><rect x="15" y="55" width="14" height="31" rx="6"/>'
    : '<rect x="15" y="14" width="14" height="72" rx="7"/>';
  return `<g fill="${colour}" color="${colour}">${stem}<path d="M77 20 46 50 77 80" fill="none" stroke="currentColor" stroke-width="14" stroke-linecap="round" stroke-linejoin="round"/></g>`;
}

export function optical(colour = "currentColor", preview = false, attention = false, size = 18) {
  if (size === 16) {
    return `<g fill="${colour}" color="${colour}"><rect x="2" y="2" width="3" height="12" rx="1"/><path d="M12.5 3 7 8 12.5 13" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></g>`;
  }
  const stem = preview
    ? '<rect x="2" y="2" width="3" height="6" rx="1"/>' +
      (attention
        ? '<path d="M3 10h1q1 0 1 1v1H3.5v2H5v1q0 1-1 1H3q-1 0-1-1v-4q0-1 1-1Z"/>'
        : '<rect x="2" y="10" width="3" height="6" rx="1"/>')
    : attention
      ? '<path d="M3.25 2h.5Q5 2 5 3.25V11H3.5v2H5v1.75Q5 16 3.75 16h-.5Q2 16 2 14.75V3.25Q2 2 3.25 2Z"/>'
      : '<rect x="2" y="2" width="3" height="14" rx="1.25"/>';
  return `<g fill="${colour}" color="${colour}">${stem}<path d="M14 3 7.25 9 14 15" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></g>`;
}
