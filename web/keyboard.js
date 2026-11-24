// The 3D 60% keyboard and the mouse in front of the cat.
// The cat faces the camera, so its keyboard is seen from the far side: the
// space row is nearest the cat and the user's left-hand keys are on screen right.
'use strict';
(() => {
  const { V } = K;
  const U = 0.15;          // one key unit in world space
  const CAP = 0.129;       // keycap footprint for a 1u key
  const ROWS = [
    [['Escape', 1, 'esc', 'orange'], ['Digit1', 1, '1'], ['Digit2', 1, '2'], ['Digit3', 1, '3'], ['Digit4', 1, '4'],
     ['Digit5', 1, '5'], ['Digit6', 1, '6'], ['Digit7', 1, '7'], ['Digit8', 1, '8'], ['Digit9', 1, '9'],
     ['Digit0', 1, '0'], ['Minus', 1, '-'], ['Equal', 1, '='], ['Backspace', 2, '⌫', 'mod']],
    [['Tab', 1.5, '⇥', 'mod'], ['KeyQ', 1, 'Q'], ['KeyW', 1, 'W'], ['KeyE', 1, 'E'], ['KeyR', 1, 'R'], ['KeyT', 1, 'T'],
     ['KeyY', 1, 'Y'], ['KeyU', 1, 'U'], ['KeyI', 1, 'I'], ['KeyO', 1, 'O'], ['KeyP', 1, 'P'],
     ['BracketLeft', 1, '['], ['BracketRight', 1, ']'], ['Backslash', 1.5, '\\', 'mod']],
    [['CapsLock', 1.75, '⇪', 'mod'], ['KeyA', 1, 'A'], ['KeyS', 1, 'S'], ['KeyD', 1, 'D'], ['KeyF', 1, 'F'],
     ['KeyG', 1, 'G'], ['KeyH', 1, 'H'], ['KeyJ', 1, 'J'], ['KeyK', 1, 'K'], ['KeyL', 1, 'L'],
     ['Semicolon', 1, ';'], ['Quote', 1, "'"], ['Enter', 2.25, '⏎', 'orange']],
    [['ShiftLeft', 2.25, '⇧', 'mod'], ['KeyZ', 1, 'Z'], ['KeyX', 1, 'X'], ['KeyC', 1, 'C'], ['KeyV', 1, 'V'],
     ['KeyB', 1, 'B'], ['KeyN', 1, 'N'], ['KeyM', 1, 'M'], ['Comma', 1, ','], ['Period', 1, '.'],
     ['Slash', 1, '/'], ['ShiftRight', 1.75, '⇧', 'mod'], ['ArrowUp', 1, '↑', 'mod']],
    [['ControlLeft', 1.25, '⌃', 'mod'], ['AltLeft', 1.25, '⌥', 'mod'], ['MetaLeft', 1.25, '⌘', 'mod'],
     ['Space', 6.25, 'space'], ['MetaRight', 1, '⌘', 'mod'], ['ArrowLeft', 1, '←', 'mod'],
     ['ArrowDown', 1, '↓', 'mod'], ['ArrowRight', 1, '→', 'mod']],
  ];
  // Aliases for keys this board has no cap for.
  const ALIAS = {
    Backquote: 'Escape', Delete: 'Backspace', Fn: 'ControlLeft', ControlRight: 'MetaRight',
    AltRight: 'MetaRight', IntlBackslash: 'ShiftLeft', NumpadEnter: 'Enter', ContextMenu: 'MetaRight',
  };
  for (let i = 1; i <= 12; i++) ALIAS['F' + i] = 'Digit' + (i === 10 ? 0 : i === 11 ? 'Minus' : i === 12 ? 'Equal' : i);
  ALIAS.F11 = 'Minus'; ALIAS.F12 = 'Equal';
  for (let i = 0; i <= 9; i++) ALIAS['Numpad' + i] = 'Digit' + i;
  Object.assign(ALIAS, { NumpadAdd: 'Equal', NumpadSubtract: 'Minus', NumpadMultiply: 'Digit8', NumpadDivide: 'Slash', NumpadDecimal: 'Period', NumpadEqual: 'Equal' });

  // Keys the user types with the left hand; the cat presses them with the paw on screen right.
  const LEFT_HAND = new Set(['Escape', 'Digit1', 'Digit2', 'Digit3', 'Digit4', 'Digit5', 'Tab', 'KeyQ', 'KeyW', 'KeyE',
    'KeyR', 'KeyT', 'CapsLock', 'KeyA', 'KeyS', 'KeyD', 'KeyF', 'KeyG', 'ShiftLeft', 'KeyZ', 'KeyX', 'KeyC', 'KeyV',
    'KeyB', 'ControlLeft', 'AltLeft', 'MetaLeft']);

  const board = new THREE.Group();
  // A little right of the cat (the mouse takes the left), far edge raised slightly like a real board.
  board.position.set(0.28, 0.02, 1.0);
  board.rotation.x = 0.06;
  K.scene.add(board);

  const W = 15 * U + 0.16, D = 5 * U + 0.16, H = 0.12;
  function slab(w, d, h, r, color, y) {
    const s = new THREE.Shape();
    const x0 = -w / 2, z0 = -d / 2;
    s.moveTo(x0 + r, z0); s.lineTo(x0 + w - r, z0); s.quadraticCurveTo(x0 + w, z0, x0 + w, z0 + r);
    s.lineTo(x0 + w, z0 + d - r); s.quadraticCurveTo(x0 + w, z0 + d, x0 + w - r, z0 + d);
    s.lineTo(x0 + r, z0 + d); s.quadraticCurveTo(x0, z0 + d, x0, z0 + d - r);
    s.lineTo(x0, z0 + r); s.quadraticCurveTo(x0, z0, x0 + r, z0);
    const g = new THREE.ExtrudeGeometry(s, { depth: h, bevelEnabled: false, curveSegments: 6 });
    g.rotateX(Math.PI / 2);
    g.translate(0, y + h, 0);
    return new THREE.Mesh(g, K.flat(color));
  }
  const caseMesh = slab(W, D, H, 0.07, '#252523', 0);
  const caseRim = slab(W + 0.05, D + 0.05, H + 0.02, 0.09, '#f4f1e6', -0.01);
  caseRim.material.side = THREE.BackSide;
  const plate = slab(W - 0.05, D - 0.05, 0.01, 0.05, '#2b2b28', H);
  board.add(caseRim, caseMesh, plate);

  // A keycap is a 4-sided frustum turned 45°, scaled to the key width: sloped sides, flat top.
  const capGeo = new THREE.CylinderGeometry(Math.SQRT1_2 * 0.8, Math.SQRT1_2, 1, 4, 1);
  capGeo.rotateY(Math.PI / 4);
  capGeo.translate(0, 0.5, 0);
  const sideMat = K.flat('#e6e3d8'), modSide = K.flat('#cbc7ba'), orangeSide = K.flat('#d39a4e');
  // A short dark notch at the near edge of each cap, like the gap between rows seen from above.
  const dotGeo = new THREE.PlaneGeometry(0.014, 0.02);
  dotGeo.rotateX(-Math.PI / 2);
  const dotMat = K.flat('#2a2a27');
  const CAP_H = 0.058;
  const COLORS = { base: new THREE.Color('#f8f7ef'), mod: new THREE.Color('#e0ddd2'), orange: new THREE.Color('#e9b064') };
  const HIT = new THREE.Color('#b9d84f');
  const keys = new Map();
  ROWS.forEach((row, r) => {
    let u = 0;
    for (const [code, width, label, kind = 'base'] of row) {
      const cx = u + width / 2;
      u += width;
      // user space → world: mirrored left/right, number row towards the camera.
      const x = (7.5 - cx) * U, z = (r - 2) * -U;
      const top = new THREE.MeshBasicMaterial({ color: COLORS[kind].clone() });
      const side = kind === 'mod' ? modSide : kind === 'orange' ? orangeSide : sideMat;
      const cap = new THREE.Mesh(capGeo, [side, top, K.flat('#1f1f1d')]);
      cap.scale.set(width * U - (U - CAP), CAP_H, CAP);
      const holder = new THREE.Group();
      holder.position.set(x, H + 0.01, z);
      const dot = new THREE.Mesh(dotGeo, dotMat);
      dot.position.set(0, CAP_H + 0.001, -CAP * 0.36);
      holder.add(cap, dot);
      board.add(holder);
      keys.set(code, { code, label, holder, top, base: COLORS[kind], x, z, down: false, glow: 0, depth: 0 });
    }
  });

  const world = new THREE.Vector3();
  K.keyboard = {
    group: board,
    releaseAll() { for (const key of keys.values()) key.down = false; },
    hitMeshes: [caseMesh, plate],
    resolve(code) { return keys.get(code) || keys.get(ALIAS[code]) || null; },
    leftHand: code => LEFT_HAND.has(ALIAS[code] || code),
    // World-space top centre of a key, including how far it is pushed in.
    keyTop(key, out = world) {
      board.updateMatrixWorld();
      return out.set(key.x, H + 0.01 + CAP_H - key.depth, key.z).applyMatrix4(board.matrixWorld);
    },
    update(dt) {
      for (const key of keys.values()) {
        const depthTarget = key.down ? 0.032 : 0;
        key.depth += (depthTarget - key.depth) * K.approach(key.down ? 60 : 24, dt);
        key.glow += ((key.down ? 1 : 0) - key.glow) * K.approach(key.down ? 40 : 5, dt);
        key.holder.position.y = H + 0.01 - key.depth;
        key.top.color.copy(key.base).lerp(HIT, key.glow);
      }
    },
  };

  // Mouse: a chubby white computer mouse lying on the desk, buttons and wheel at the front
  // (towards the viewer, the way the cat holds it), palm hump at the back under the cat's paw,
  // a dark sticker outline, and a cable trailing off to the side.
  const mouse = new THREE.Group();
  const MOUSE_HOME = V(-1.2, 0.02, 0.96);
  mouse.position.copy(MOUSE_HOME);
  // How far the mouse may slide right before its outline (0.19 from its centre, click squash
  // included) would touch the keyboard's cream edge.
  const MOUSE_ROOM = board.position.x - (W + 0.05) / 2 - 0.19 - MOUSE_HOME.x;
  const shape = new THREE.Group();
  shape.rotation.y = -0.18;
  mouse.add(shape);
  const MW = 0.145, MH = 0.1, ML = 0.21;
  const domeGeo = new THREE.SphereGeometry(1, 40, 24, 0, Math.PI * 2, 0, Math.PI / 2);
  {
    // Egg in plan (fuller at the back), highest just behind the middle, with a little side shading.
    const q = domeGeo.attributes.position, colors = [];
    const top = new THREE.Color('#fbfbf3'), side = new THREE.Color('#dedad0'), c = new THREE.Color();
    for (let i = 0; i < q.count; i++) {
      const x = q.getX(i), y = q.getY(i), z = q.getZ(i);
      q.setXYZ(i, x * (1 - 0.14 * z), y * (1 - 0.18 * z), z);
      c.copy(side).lerp(top, K.smooth(0.15, 0.75, y));
      colors.push(c.r, c.g, c.b);
    }
    domeGeo.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
  }
  // 3D style shades the sides from the vertex colours; 2D is one flat tone.
  const bodyMat = new THREE.MeshBasicMaterial({ vertexColors: true, color: '#ffffff' });
  const dome = new THREE.Mesh(domeGeo, bodyMat);
  dome.scale.set(MW, MH, ML);
  const shell = (color, grow, order) => {
    const m = new THREE.Mesh(domeGeo, K.back(color));
    m.scale.set(MW + grow, MH + grow, ML + grow);
    m.position.y = -grow * 0.3;
    m.renderOrder = order;
    return m;
  };
  // Seam between the two buttons along the front half, and the scroll wheel in it.
  const onTop = (x, z) => MH * (1 - 0.18 * z / ML) * Math.sqrt(Math.max(0, 1 - (x / (MW * (1 - 0.14 * z / ML))) ** 2 - (z / ML) ** 2));
  const seam = new K.TaperTube(12, 4, new THREE.MeshBasicMaterial({ color: '#2a2a27', depthTest: false }), 6);
  seam.update((t, out) => { const z = K.mix(ML * 0.02, ML * 0.95, t); return out.set(0, onTop(0, z) + 0.002, z); }, () => 0.0045);
  const wheel = new THREE.Mesh(new THREE.SphereGeometry(1, 16, 10), new THREE.MeshBasicMaterial({ color: '#e9a94f', depthTest: false }));
  wheel.scale.set(0.016, 0.012, 0.03);
  wheel.position.set(0, onTop(0, ML * 0.5) + 0.004, ML * 0.5);
  wheel.renderOrder = 7;
  // The left button lights up on a click, like the keys do.
  const buttonGeo = new THREE.CircleGeometry(1, 24, 0, Math.PI / 2);
  buttonGeo.rotateX(Math.PI / 2);
  const buttonMat = new THREE.MeshBasicMaterial({ color: '#b9d84f', transparent: true, opacity: 0, depthTest: false });
  const button = new THREE.Mesh(buttonGeo, buttonMat);
  button.scale.set(MW * 0.78, 1, ML * 0.82);
  button.position.set(0.004, onTop(0.04, ML * 0.4) + 0.001, ML * 0.08);
  button.renderOrder = 5;
  shape.add(shell('#f4f1e6', 0.03, -2), shell('#1d1d1b', 0.013, -1), dome, button, seam.mesh, wheel);
  K.scene.add(mouse);
  // Cable: leaves the front of the mouse and curls away to the left, off the desk.
  const cable = new K.TaperTube(24, 6, K.flat('#2a2a27'), 1);
  K.scene.add(cable.mesh);
  const cableStart = V(0, 0, 0), cableEnd = V(-1.6, 0.02, 1.7), cableMid = V(0, 0, 0);
  const faceTop = V(0, 0, 0);
  K.mouseProp = {
    group: mouse,
    home: MOUSE_HOME,
    hitMeshes: [dome],
    offset: V(0, 0, 0),
    kick: V(0, 0, 0),
    pressed: 0,
    // The disc follows the real mouse with a little lag and slides back home when let go.
    follow: new K.Spring(V(0, 0, 0), 4, 0.75),
    update(dt, down) {
      this.kick.multiplyScalar(1 - K.approach(3, dt));
      this.offset.copy(this.follow.step(this.kick, dt));
      // Rightward travel eases off as it nears the keyboard instead of running into it.
      if (this.offset.x > 0) this.offset.x = MOUSE_ROOM * Math.tanh(this.offset.x / MOUSE_ROOM);
      mouse.position.copy(MOUSE_HOME).add(this.offset);
      this.pressed += ((down ? 1 : 0) - this.pressed) * K.approach(30, dt);
      buttonMat.opacity = 0.9 * this.pressed;
      // A click squashes the mouse a touch under the paw.
      shape.scale.set(1 + 0.03 * this.pressed, 1 - 0.1 * this.pressed, 1);
      // The cable hangs from the front of the mouse and sags on its way off the desk.
      mouse.updateMatrixWorld();
      cableStart.set(0, MH * 0.25, ML * 0.98).applyMatrix4(shape.matrixWorld);
      cableMid.set((cableStart.x + cableEnd.x) / 2 + 0.1, 0.02, (cableStart.z + cableEnd.z) / 2 + 0.12);
      cable.update((t, o) => o.set(
        (1 - t) ** 2 * cableStart.x + 2 * (1 - t) * t * cableMid.x + t * t * cableEnd.x,
        (1 - t) ** 2 * cableStart.y + 2 * (1 - t) * t * cableMid.y + t * t * cableEnd.y,
        (1 - t) ** 2 * cableStart.z + 2 * (1 - t) * t * cableMid.z + t * t * cableEnd.z), () => 0.011);
    },
    setStyle(style) {
      const flat = style !== '3d';
      bodyMat.vertexColors = !flat;
      bodyMat.color.set(flat ? '#f8f7f0' : '#ffffff');
      bodyMat.needsUpdate = true;
    },
    // Where a paw rests on the mouse, in world space: on the palm hump at the back, so the
    // buttons, wheel and cable stay in view in front of it.
    top(out) {
      mouse.updateMatrixWorld();
      return out.copy(faceTop.set(0, MH + 0.045, -ML * 0.3)).applyMatrix4(shape.matrixWorld);
    },
  };
})();
