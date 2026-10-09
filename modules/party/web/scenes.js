// Party Games: the preset visual-novel backgrounds for Drama Club, drawn as flat SVG (all
// original). PartyScenes.svg(key) returns a 1280x720 scene; PartyScenes.LABELS names them.
// The keys match BACKGROUNDS in partygames.py.
(() => {
  'use strict';
  const W = 1280;
  const H = 720;
  const rect = (x, y, w, h, fill, extra = '') =>
    `<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${fill}" ${extra}/>`;
  const sky = (id, top, bottom) =>
    `<defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${top}"/><stop offset="1" stop-color="${bottom}"/></linearGradient></defs>${rect(0, 0, W, H, `url(#${id})`)}`;
  const range = (n, f) => Array.from({ length: n }, (_, i) => f(i)).join('');
  const stars = (n, seed, color = '#fff') =>
    range(n, (i) => {
      const x = (seed * 97 + i * 211) % W;
      const y = (seed * 53 + i * 89) % 320;
      return `<circle cx="${x}" cy="${y}" r="${1 + (i % 3)}" fill="${color}" opacity="${0.4 + (i % 5) / 10}"/>`;
    });

  const SCENES = {
    classroom: () =>
      rect(0, 0, W, H, '#f3e6c8') +
      rect(0, 520, W, 200, '#b9875a') +
      range(12, (i) => rect(i * 110, 520, 4, 200, '#a57449')) +
      rect(140, 90, 620, 300, '#2f5d46', 'rx="10"') +
      rect(130, 80, 640, 320, 'none', 'rx="14" stroke="#7b5534" stroke-width="18"') +
      '<path d="M200 170 q60 -40 120 0 t120 0" stroke="#e8f0e8" stroke-width="5" fill="none" opacity=".7"/><text x="200" y="300" font-size="44" fill="#e8f0e8" opacity=".65" font-family="cursive">Homework: be dramatic</text>' +
      rect(860, 70, 330, 380, '#bfe3ff', 'rx="6"') +
      rect(1018, 70, 14, 380, '#fff') +
      rect(860, 252, 330, 14, '#fff') +
      rect(850, 60, 350, 400, 'none', 'stroke="#fff" stroke-width="16" rx="8"') +
      '<circle cx="960" cy="150" r="40" fill="#fff6b0"/>' +
      range(
        3,
        (i) =>
          rect(120 + i * 380, 560, 260, 26, '#8a5a35', 'rx="6"') +
          rect(150 + i * 380, 586, 20, 110, '#6e4527') +
          rect(330 + i * 380, 586, 20, 110, '#6e4527'),
      ),
    rooftop: () =>
      sky('rt', '#ff8a5c', '#ffd27a') +
      '<circle cx="980" cy="430" r="120" fill="#fff1b8" opacity=".9"/>' +
      range(9, (i) =>
        rect(
          i * 150 - 20,
          300 + ((i * 37) % 120),
          120,
          400,
          ['#6b4b7a', '#7d5a8a', '#5c3f6b'][i % 3],
        ),
      ) +
      range(30, (i) =>
        rect(((i * 47) % 1240) + 10, 340 + ((i * 61) % 260), 14, 18, '#ffdf8a', 'opacity=".8"'),
      ) +
      rect(0, 560, W, 160, '#8d8fa3') +
      range(17, (i) => rect(i * 80, 470, 8, 100, '#3d3a4f')) +
      rect(0, 470, W, 10, '#3d3a4f') +
      rect(0, 530, W, 8, '#3d3a4f'),
    cafe: () =>
      rect(0, 0, W, H, '#f6dcc1') +
      rect(0, 0, W, 60, '#7a4b33') +
      range(
        14,
        (i) => `<path d="M${i * 92} 60 l46 40 l46 -40z" fill="${i % 2 ? '#e86f5a' : '#fff'}"/>`,
      ) +
      rect(80, 160, 420, 260, '#cfeaff', 'rx="8"') +
      rect(70, 150, 440, 280, 'none', 'stroke="#7a4b33" stroke-width="16" rx="10"') +
      rect(700, 150, 480, 180, '#4a3326', 'rx="10"') +
      '<text x="740" y="220" font-size="40" fill="#fbe9d0" font-family="cursive">Today: latte art</text><text x="740" y="290" font-size="32" fill="#fbe9d0" opacity=".8" font-family="cursive">Cake of the day ♥</text>' +
      rect(0, 520, W, 200, '#9c6b47') +
      rect(600, 440, 640, 100, '#7a4b33', 'rx="8"') +
      range(
        4,
        (i) =>
          `<circle cx="${700 + i * 140}" cy="425" r="22" fill="${['#fff', '#e86f5a', '#ffd27a', '#8fd1a8'][i]}"/>`,
      ) +
      '<ellipse cx="240" cy="560" rx="140" ry="18" fill="#7a4b33"/>' +
      rect(232, 560, 16, 130, '#5e3a25'),
    bedroom: () =>
      sky('br', '#2b2453', '#4a3a7a') +
      rect(100, 100, 360, 300, '#121037', 'rx="10"') +
      stars(14, 3) +
      '<circle cx="370" cy="190" r="50" fill="#fff4c4"/><circle cx="392" cy="176" r="46" fill="#121037"/>' +
      rect(90, 90, 380, 320, 'none', 'stroke="#e9d7ff" stroke-width="14" rx="10"') +
      rect(0, 540, W, 180, '#3a2f63') +
      rect(640, 400, 520, 180, '#e7a1c0', 'rx="30"') +
      rect(640, 380, 160, 90, '#fff', 'rx="30"') +
      rect(640, 330, 30, 260, '#6b4b8a', 'rx="8"') +
      '<circle cx="1010" cy="250" r="70" fill="#ffe08a" opacity=".25"/>' +
      rect(990, 250, 40, 150, '#d8c7ff') +
      `<path d="M960 250 h100 l-25 -70 h-50z" fill="#ffd27a"/>` +
      range(5, (i) =>
        rect(
          560 + i * 18,
          160 + (i % 2) * 6,
          14,
          120 - (i % 3) * 18,
          ['#e86f5a', '#8fd1a8', '#ffd27a', '#7ab8ff', '#ff9ac1'][i],
        ),
      ) +
      rect(540, 280, 120, 12, '#8a6bb0'),
    park: () =>
      sky('pk', '#9fd8ff', '#e6f6ff') +
      '<ellipse cx="300" cy="620" rx="700" ry="200" fill="#8fd18a"/><ellipse cx="1100" cy="650" rx="600" ry="180" fill="#7cc579"/>' +
      range(2, (i) => {
        const x = 220 + i * 760;
        return `${rect(x - 18, 300, 36, 280, '#7a5236')}<circle cx="${x}" cy="260" r="130" fill="#ffc1d9"/><circle cx="${x - 90}" cy="320" r="90" fill="#ffb0cf"/><circle cx="${x + 90}" cy="320" r="90" fill="#ffd3e4"/>`;
      }) +
      range(
        26,
        (i) =>
          `<ellipse cx="${(i * 97) % W}" cy="${380 + ((i * 53) % 300)}" rx="7" ry="4" fill="#ffc1d9" transform="rotate(${i * 23} ${(i * 97) % W} ${380 + ((i * 53) % 300)})"/>`,
      ) +
      rect(520, 540, 300, 20, '#8a5a35', 'rx="6"') +
      rect(520, 500, 300, 16, '#8a5a35', 'rx="6"') +
      rect(540, 560, 14, 60, '#5e3a25') +
      rect(786, 560, 14, 60, '#5e3a25'),
    beach: () =>
      sky('bc', '#7ccfff', '#d9f3ff') +
      '<circle cx="1080" cy="140" r="70" fill="#fff3a6"/>' +
      rect(0, 380, W, 160, '#2fa8d8') +
      range(
        8,
        (i) =>
          `<path d="M${i * 170} 420 q40 -18 80 0" stroke="#fff" stroke-width="5" fill="none" opacity=".7"/>`,
      ) +
      '<path d="M0 520 Q640 470 1280 520 V720 H0z" fill="#f3d79c"/>' +
      '<path d="M200 520 L230 300" stroke="#8a5a35" stroke-width="14"/><path d="M230 300 q-90 -10 -140 40 M230 300 q80 -40 150 0 M230 300 q-30 -60 -100 -60 M230 300 q40 -70 110 -50" stroke="#2fae6a" stroke-width="22" fill="none" stroke-linecap="round"/>' +
      '<path d="M900 640 l90 -200 l90 200z" fill="#ff7a7a"/><path d="M990 440 v200" stroke="#fff" stroke-width="6"/>' +
      '<ellipse cx="560" cy="650" rx="110" ry="16" fill="#5bc0eb"/>',
    street: () =>
      sky('st', '#4b5878', '#7f8aa8') +
      range(
        6,
        (i) =>
          rect(
            i * 230 - 20,
            150 + ((i * 41) % 90),
            200,
            420,
            ['#3c4560', '#46506d', '#353d55'][i % 3],
          ) +
          range(6, (j) =>
            rect(
              i * 230 + 10 + (j % 2) * 90,
              190 + ((i * 41) % 90) + Math.floor(j / 2) * 110,
              60,
              70,
              j % 3 ? '#ffd98a' : '#2a3048',
            ),
          ),
      ) +
      rect(0, 560, W, 160, '#2b2f3d') +
      range(8, (i) => rect(60 + i * 170, 630, 80, 10, '#f2f2f2', 'opacity=".6"')) +
      rect(1040, 260, 12, 320, '#1e2230') +
      '<circle cx="1046" cy="252" r="26" fill="#ffe8a8"/><ellipse cx="1046" cy="600" rx="90" ry="14" fill="#ffe8a8" opacity=".25"/>' +
      range(
        60,
        (i) =>
          `<path d="M${(i * 71) % W} ${(i * 37) % H} l-8 22" stroke="#cfe0ff" stroke-width="2" opacity=".55"/>`,
      ),
    train: () =>
      rect(0, 0, W, H, '#dfe7ef') +
      rect(0, 0, W, 70, '#b9c7d6') +
      range(
        4,
        (i) =>
          rect(60 + i * 310, 130, 260, 220, '#8fd0ff', 'rx="22"') +
          `<path d="M${60 + i * 310} 300 l70 -60 l60 40 l80 -70 l50 40 v90 h-260z" fill="#7cc579"/>` +
          rect(55 + i * 310, 125, 270, 230, 'none', 'stroke="#9aa9ba" stroke-width="12" rx="24"'),
      ) +
      rect(0, 400, W, 40, '#6f7f93') +
      range(
        5,
        (i) =>
          rect(40 + i * 250, 440, 210, 150, '#3f6db5', 'rx="18"') +
          rect(40 + i * 250, 430, 210, 40, '#5584cf', 'rx="16"'),
      ) +
      rect(0, 600, W, 120, '#9aa6b4') +
      range(
        16,
        (i) =>
          `<path d="M${i * 82 + 40} 70 v40" stroke="#6f7f93" stroke-width="4"/><circle cx="${i * 82 + 40}" cy="118" r="10" fill="none" stroke="#6f7f93" stroke-width="4"/>`,
      ),
    festival: () =>
      sky('fs', '#1f1846', '#4b2f6e') +
      stars(10, 7) +
      range(
        3,
        (i) =>
          `<circle cx="${250 + i * 380}" cy="${120 + (i % 2) * 40}" r="${50 + i * 8}" fill="none" stroke="${['#ff7aa8', '#ffd27a', '#7ae0ff'][i]}" stroke-width="5" stroke-dasharray="6 14" opacity=".9"/>`,
      ) +
      '<path d="M0 230 Q320 290 640 230 T1280 230" stroke="#2b2240" stroke-width="4" fill="none"/>' +
      range(11, (i) => {
        const x = 60 + i * 116;
        const y = 248 + Math.round(28 * Math.sin((i / 10) * Math.PI * 2));
        return `<ellipse cx="${x}" cy="${y + 30}" rx="26" ry="34" fill="${i % 2 ? '#ff5a5f' : '#ffb100'}"/><ellipse cx="${x}" cy="${y + 30}" rx="40" ry="48" fill="#ffd27a" opacity=".18"/>`;
      }) +
      range(
        3,
        (i) =>
          rect(80 + i * 420, 430, 300, 170, ['#e8524a', '#3d7bff', '#2ec27e'][i], 'rx="8"') +
          rect(70 + i * 420, 400, 320, 50, '#fff', 'rx="8"') +
          range(5, (j) =>
            rect(70 + i * 420 + j * 64, 400, 32, 50, ['#e8524a', '#3d7bff', '#2ec27e'][i]),
          ),
      ) +
      rect(0, 600, W, 120, '#2b2240'),
    castle: () =>
      rect(0, 0, W, H, '#5b4a7a') +
      range(5, (i) => rect(80 + i * 250, 0, 80, 560, '#6d5b8f')) +
      range(
        4,
        (i) =>
          `<path d="M${200 + i * 250} 380 v-200 a60 60 0 0 1 120 0 v200z" fill="#9fc7ff"/><path d="M${260 + i * 250} 120 v260 M${200 + i * 250} 250 h120" stroke="#5b4a7a" stroke-width="8"/>`,
      ) +
      rect(0, 560, W, 160, '#3e3258') +
      '<path d="M540 720 L600 560 H680 L740 720z" fill="#c4363d"/>' +
      range(
        2,
        (i) =>
          `${rect(160 + i * 900, 300, 12, 90, '#c9a14a')}<path d="M${166 + i * 900} 300 q-18 -30 0 -60 q18 30 0 60z" fill="#ffb100"/>`,
      ) +
      range(
        2,
        (i) =>
          `<path d="M${440 + i * 330} 60 v140 l40 -30 l40 30 v-140z" fill="${i ? '#3d7bff' : '#c4363d'}"/>`,
      ) +
      '<circle cx="640" cy="60" r="0"/>',
    spaceship: () =>
      rect(0, 0, W, H, '#151a2e') +
      '<path d="M140 80 H1140 L1240 400 H40z" fill="#060912"/>' +
      stars(40, 11, '#cfe3ff') +
      '<circle cx="980" cy="230" r="90" fill="#ff8a5c"/><ellipse cx="980" cy="230" rx="150" ry="24" fill="none" stroke="#ffd27a" stroke-width="6"/>' +
      '<path d="M140 80 H1140 L1240 400 H40z" fill="none" stroke="#5a6688" stroke-width="18"/>' +
      rect(0, 400, W, 320, '#2a3150') +
      rect(200, 470, 880, 120, '#3a4470', 'rx="20"') +
      range(12, (i) =>
        rect(
          240 + i * 68,
          500,
          40,
          24,
          ['#2ec27e', '#ff5a5f', '#ffb100', '#3d7bff'][i % 4],
          'rx="6"',
        ),
      ) +
      range(
        10,
        (i) =>
          `<circle cx="${260 + i * 80}" cy="560" r="10" fill="${i % 3 ? '#7ae0ff' : '#ff7aa8'}"/>`,
      ) +
      rect(0, 640, W, 80, '#1f253d'),
    haunted: () =>
      sky('hh', '#1b2333', '#3a3355') +
      '<circle cx="1050" cy="140" r="80" fill="#f2f0d8"/>' +
      range(
        3,
        (i) =>
          `<path d="M${880 + i * 70} ${110 + i * 30} q15 -15 30 0 q15 -15 30 0" stroke="#1b2333" stroke-width="5" fill="none"/>`,
      ) +
      '<path d="M260 620 V300 L440 160 L620 300 V620z" fill="#2a2338"/><path d="M220 310 L440 130 L660 310" stroke="#1a1526" stroke-width="26" fill="none"/>' +
      range(4, (i) =>
        rect(
          300 + (i % 2) * 190,
          330 + Math.floor(i / 2) * 140,
          90,
          90,
          i === 1 ? '#ffd86b' : '#4c4466',
          'rx="4"',
        ),
      ) +
      rect(400, 520, 80, 100, '#14101e', 'rx="40"') +
      '<path d="M760 620 V420 q20 -60 40 0 v200 M840 620 V380 q20 -60 40 0 v240" stroke="#2a2338" stroke-width="14" fill="none"/>' +
      rect(0, 600, W, 120, '#232030') +
      '<path d="M960 560 q30 -90 60 0 q-10 20 -20 0 q-10 20 -20 0 q-10 20 -20 0z" fill="#f2f0f8" opacity=".85"/><circle cx="980" cy="510" r="5" fill="#232030"/><circle cx="1000" cy="510" r="5" fill="#232030"/>' +
      range(6, (i) => `<path d="M${100 + i * 200} 680 l10 -50 l10 50z" fill="#3d3550"/>`),
  };
  const LABELS = {
    classroom: 'Classroom',
    rooftop: 'School rooftop',
    cafe: 'Café',
    bedroom: 'Bedroom at night',
    park: 'Cherry blossom park',
    beach: 'Beach',
    street: 'Rainy street',
    train: 'Train',
    festival: 'Summer festival',
    castle: 'Castle hall',
    spaceship: 'Spaceship bridge',
    haunted: 'Haunted house',
  };
  // Picture backgrounds served from /bg/ (in web/bg; see web/bg/CREDITS.md), listed first.
  const PHOTOS = {
    classroom_day: 'Classroom (day)',
    school_hallway: 'School courtyard',
    bedroom_day: 'Bedroom (day)',
    livingroom_night: 'Living room',
    kitchen_day: 'Kitchen',
    restaurant: 'Restaurant',
    city_afternoon: 'City crossing',
    spring_street: 'Spring evening street',
    train_beach: 'Train by the sea',
    onsen: 'Hot spring',
  };
  /** The scene as an <svg> string (falls back to the classroom). small: a thumbnail, for phones. */
  const svg = (key, cls = '', small = false) => {
    const inner =
      key in PHOTOS
        ? `<image href="/bg/${key}${small ? '.thumb' : ''}.webp" width="${W}" height="${H}" preserveAspectRatio="xMidYMid slice"/>`
        : (SCENES[key] || SCENES.classroom)();
    return `<svg class="${cls}" viewBox="0 0 ${W} ${H}" preserveAspectRatio="xMidYMid slice" aria-hidden="true">${inner}</svg>`;
  };
  window.PartyScenes = {
    svg,
    LABELS: { ...PHOTOS, ...LABELS },
    KEYS: [...Object.keys(PHOTOS), ...Object.keys(SCENES)],
  };
})();
