<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { currentMonitor, cursorPosition, getCurrentWindow, primaryMonitor } from '@tauri-apps/api/window';

  type PetState = 'idle' | 'look' | 'click' | 'sleep' | string;
  type PetMode = 'leisure' | 'patrol' | 'interactive';

  type AnimationItem = {
    src: string;
    framePattern?: string;
    frameCount?: number;
    fps?: number;
    alt?: string;
    loop?: boolean;
    durationMs?: number;
    nextState?: PetState;
  };

  type AnimationConfig = {
    defaultState: PetState;
    defaultMode: PetMode;
    assetVersion?: number;
    sleepAfterMs: number;
    leisureSleepAfterMs?: number;
    movement: {
      enabled: boolean;
      speedPxPerSecond: number;
      bottomMarginPx: number;
      updateMs: number;
      pauseInStates: PetState[];
    };
    interaction?: {
      pollMs: number;
      approachRadiusPx: number;
      hoverDelayMs: number;
      fastPointerSpeed: number;
      actionLockMs: number;
      clickWindowMs: number;
      mediumClickCount: number;
      heavyClickCount: number;
    };
    states: Record<PetState, AnimationItem>;
  };

  const fallbackConfig: AnimationConfig = {
    defaultState: 'idle',
    defaultMode: 'leisure',
    assetVersion: 4,
    sleepAfterMs: 30_000,
    leisureSleepAfterMs: 15_000,
    movement: {
      enabled: true,
      speedPxPerSecond: 55,
      bottomMarginPx: 12,
      updateMs: 33,
      pauseInStates: ['sleep', 'click']
    },
    interaction: {
      pollMs: 80,
      approachRadiusPx: 180,
      hoverDelayMs: 900,
      fastPointerSpeed: 1100,
      actionLockMs: 900,
      clickWindowMs: 1400,
      mediumClickCount: 4,
      heavyClickCount: 7
    },
    states: {
      idle: {
        src: '/assets/cat/play_frames/frame_001.png',
        framePattern: '/assets/cat/play_frames/frame_{index}.png',
        frameCount: 100,
        fps: 10,
        alt: 'Idle cat',
        loop: true
      },
      look: {
        src: '/assets/cat/click_frames/frame_001.png',
        framePattern: '/assets/cat/click_frames/frame_{index}.png',
        frameCount: 100,
        fps: 10,
        alt: 'Cat looking at pointer',
        loop: true
      },
      click: {
        src: '/assets/cat/click_frames/frame_001.png',
        framePattern: '/assets/cat/click_frames/frame_{index}.png',
        frameCount: 100,
        fps: 10,
        alt: 'Clicked cat',
        loop: true
      },
      sleep: {
        src: '/assets/cat/sleep_frames/frame_001.png',
        framePattern: '/assets/cat/sleep_frames/frame_{index}.png',
        frameCount: 100,
        fps: 10,
        alt: 'Sleeping cat',
        loop: true
      }
    }
  };

  const appWindow = getCurrentWindow();
  const pointerLookRadius = 96;
  const dragThreshold = 6;
  const contextMenuWidth = 104;
  const contextMenuHeight = 86;
  const scaleStep = 0.02;
  const minPetScale = 0.5;
  const maxPetScale = 2;

  const modeLabels: Record<PetMode, string> = {
    leisure: '休闲模式',
    patrol: '巡回模式',
    interactive: '交互模式'
  };

  let config = $state<AnimationConfig>(fallbackConfig);
  let currentMode = $state<PetMode>(fallbackConfig.defaultMode);
  let currentState = $state<PetState>(fallbackConfig.defaultState);
  let contextMenu = $state<{ x: number; y: number } | null>(null);
  let isDragging = $state(false);
  let dragLocked = $state(false);
  let pointerDownAt = $state<{ x: number; y: number } | null>(null);
  let walkX: number | undefined;
  let walkY: number | undefined;
  let walkDirection = 1;
  let isPositioning = false;
  let movementEpoch = 0;
  let clickTimer: ReturnType<typeof setTimeout> | undefined;
  let sleepTimer: ReturnType<typeof setTimeout> | undefined;
  let movementTimer: ReturnType<typeof setInterval> | undefined;
  let frameTimer: ReturnType<typeof setInterval> | undefined;
  let dragReleaseTimer: ReturnType<typeof setTimeout> | undefined;
  let currentFrame = $state(0);
  let mediaElement = $state<HTMLImageElement | HTMLVideoElement | null>(null);
  let lastMoveAt = performance.now();
  let lastDiagnosticKey = '';
  let lastAnimationKey = '';
  let preloadEpoch = 0;
  let activeFrameUrls = $state<string[]>([]);
  let petScale = $state(1);
  let baseWindowSize: { width: number; height: number } | null = null;
  let resizeQueue = Promise.resolve();
  let resizeCenter: { x: number; y: number } | null = null;
  let resizeSession = 0;
  let resizeCenterTimer: ReturnType<typeof setTimeout> | undefined;
  let leisureWakeClickTimer: ReturnType<typeof setTimeout> | undefined;
  let interactionTimer: ReturnType<typeof setInterval> | undefined;
  let interactionResetTimer: ReturnType<typeof setTimeout> | undefined;
  let interactionHoverTimer: ReturnType<typeof setTimeout> | undefined;
  let interactionPhase: 'away' | 'near' | 'inside' | 'hover' = 'away';
  let interactionActionUntil = 0;
  let interactionOffsetX = $state(0);
  let interactionOffsetY = $state(0);
  let lastCursorSample: { x: number; y: number; time: number } | null = null;
  let interactiveClicks: number[] = [];

  const interactiveDefaults = fallbackConfig.interaction!;

  let currentAnimation = $derived(
    config.states[currentState] ?? config.states[config.defaultState] ?? fallbackConfig.states.idle
  );
  let isVideo = $derived(/\.(webm|mp4|mov)$/i.test(currentAnimation.src));
  let isFrameSequence = $derived(Boolean(currentAnimation.framePattern && currentAnimation.frameCount));
  let frameSrc = $derived(
    isFrameSequence
      ? (activeFrameUrls[currentFrame] ?? formatFrameSrc(currentAnimation.framePattern as string, currentFrame + 1))
      : currentAnimation.src
  );
  let canAutoWalk = $derived(
    currentMode === 'patrol' &&
      config.movement.enabled &&
      !isDragging &&
      !dragLocked &&
      !pointerDownAt &&
      !config.movement.pauseInStates.includes(currentState)
  );

  function setPetState(nextState: PetState) {
    if (!config.states[nextState]) return;
    currentState = nextState;
    void logRenderDiagnostics(`state:${nextState}`);

    if (clickTimer) clearTimeout(clickTimer);
    const nextAnimation = config.states[nextState];
    if (nextAnimation.durationMs && nextAnimation.nextState) {
      clickTimer = setTimeout(() => setPetState(nextAnimation.nextState as PetState), nextAnimation.durationMs);
    }
  }

  function interactionSettings() {
    return config.interaction ?? interactiveDefaults;
  }

  function playInteractiveAction(states: PetState[], durationMs = interactionSettings().actionLockMs) {
    if (currentMode !== 'interactive') return;
    const requestedState = states[Math.floor(Math.random() * states.length)];
    const activeState = config.states[requestedState]
      ? requestedState
      : ['pounce', 'chase', 'rapid_swat', 'tail_annoyed', 'paw_back', 'fur_puff'].includes(requestedState)
        ? 'play'
        : 'click';
    interactionActionUntil = performance.now() + durationMs;
    setPetState(activeState);
    if (interactionResetTimer) clearTimeout(interactionResetTimer);
    interactionResetTimer = setTimeout(() => {
      if (currentMode !== 'interactive') return;
      interactionActionUntil = 0;
      setPetState(interactionPhase === 'away' ? 'play' : config.states.look_mouse ? 'look_mouse' : 'look');
    }, durationMs);
    logDiagnostic('interactive-action', { action: requestedState, state: activeState, durationMs });
  }

  function setTrackingOffset(cursorX: number, cursorY: number, centerX: number, centerY: number) {
    const dx = cursorX - centerX;
    const dy = cursorY - centerY;
    const distance = Math.max(1, Math.hypot(dx, dy));
    const strength = Math.min(10, distance * 0.055);
    interactionOffsetX = (dx / distance) * strength;
    interactionOffsetY = (dy / distance) * Math.min(7, strength);
  }

  async function sampleInteractivePointer() {
    if (currentMode !== 'interactive' || isDragging || dragLocked) return;
    const [cursor, position, size] = await Promise.all([
      cursorPosition(),
      appWindow.outerPosition(),
      appWindow.outerSize()
    ]);
    if (currentMode !== 'interactive') return;

    const now = performance.now();
    const centerX = position.x + size.width / 2;
    const centerY = position.y + size.height / 2;
    const dx = Math.max(position.x - cursor.x, 0, cursor.x - (position.x + size.width));
    const dy = Math.max(position.y - cursor.y, 0, cursor.y - (position.y + size.height));
    const distanceToWindow = Math.hypot(dx, dy);
    const inside = dx === 0 && dy === 0;
    const near = distanceToWindow <= interactionSettings().approachRadiusPx;
    const speed = lastCursorSample
      ? (Math.hypot(cursor.x - lastCursorSample.x, cursor.y - lastCursorSample.y) * 1000) /
        Math.max(1, now - lastCursorSample.time)
      : 0;
    lastCursorSample = { x: cursor.x, y: cursor.y, time: now };

    if (near) setTrackingOffset(cursor.x, cursor.y, centerX, centerY);
    else {
      interactionOffsetX = 0;
      interactionOffsetY = 0;
    }

    if (near && speed >= interactionSettings().fastPointerSpeed && now >= interactionActionUntil) {
      playInteractiveAction(['pounce', 'chase', 'rapid_swat'], 1100);
      return;
    }

    const nextPhase = inside ? 'inside' : near ? 'near' : 'away';
    if (nextPhase === interactionPhase || now < interactionActionUntil) return;
    interactionPhase = nextPhase;
    if (interactionHoverTimer) clearTimeout(interactionHoverTimer);
    interactionHoverTimer = undefined;

    if (nextPhase === 'away') {
      setPetState('play');
      return;
    }
    const trackingState = nextPhase === 'near' ? 'look_mouse' : 'follow_mouse';
    setPetState(config.states[trackingState] ? trackingState : 'look');
    if (inside) {
      interactionHoverTimer = setTimeout(() => {
        if (currentMode === 'interactive' && interactionPhase === 'inside') {
          interactionPhase = 'hover';
          playInteractiveAction(['observe', 'paw_probe', 'head_tilt'], 1300);
        }
      }, interactionSettings().hoverDelayMs);
    }
  }

  function formatFrameSrc(pattern: string, frameNumber: number) {
    const src = pattern.replace('{index}', String(frameNumber).padStart(3, '0'));
    const separator = src.includes('?') ? '&' : '?';
    return `${src}${separator}v=${config.assetVersion ?? 1}`;
  }

  function logDiagnostic(event: string, payload: Record<string, unknown> = {}) {
    const message = JSON.stringify({
      event,
      time: new Date().toISOString(),
      mode: currentMode,
      state: currentState,
      frame: currentFrame + 1,
      animation: {
        src: currentAnimation.src,
        framePattern: currentAnimation.framePattern,
        frameCount: currentAnimation.frameCount,
        fps: currentAnimation.fps,
        resolvedSrc: frameSrc,
        isFrameSequence,
        isVideo
      },
      movement: {
        walkX,
        walkY,
        isDragging,
        dragLocked,
        pointerDown: Boolean(pointerDownAt),
        movementEpoch
      },
      ...payload
    });

    console.info('[desktop-pet]', message);
    void invoke<string>('write_frontend_log', { message }).catch((error) => {
      console.warn('[desktop-pet] write_frontend_log failed', error);
    });
  }

  function computedStyleSummary(selector: string) {
    const element = document.querySelector(selector);
    if (!element) return null;
    const style = window.getComputedStyle(element);
    return {
      selector,
      background: style.background,
      backgroundColor: style.backgroundColor,
      opacity: style.opacity,
      display: style.display,
      width: style.width,
      height: style.height
    };
  }

  async function inspectFrameAlpha(src: string) {
    const image = new Image();
    image.decoding = 'async';
    image.src = src;
    await image.decode();

    const canvas = document.createElement('canvas');
    canvas.width = image.naturalWidth;
    canvas.height = image.naturalHeight;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (!context) return { error: '2d context unavailable' };

    context.clearRect(0, 0, canvas.width, canvas.height);
    context.drawImage(image, 0, 0);

    const imageData = context.getImageData(0, 0, canvas.width, canvas.height).data;
    function readPixel(x: number, y: number) {
      const index = (y * canvas.width + x) * 4;
      const r = imageData[index];
      const g = imageData[index + 1];
      const b = imageData[index + 2];
      const a = imageData[index + 3];
      return { x, y, r, g, b, a };
    }

    const edgePoints: Array<[number, number]> = [];
    const stepX = Math.max(1, Math.floor(canvas.width / 10));
    const stepY = Math.max(1, Math.floor(canvas.height / 10));
    for (let x = 0; x < canvas.width; x += stepX) {
      edgePoints.push([x, 0], [x, canvas.height - 1]);
    }
    for (let y = 0; y < canvas.height; y += stepY) {
      edgePoints.push([0, y], [canvas.width - 1, y]);
    }

    let transparentEdges = 0;
    let opaqueWhiteEdges = 0;
    const samplePixels = edgePoints.map(([x, y]) => {
      const pixel = readPixel(x, y);
      if (pixel.a === 0) transparentEdges += 1;
      if (pixel.a > 240 && pixel.r > 245 && pixel.g > 245 && pixel.b > 245) opaqueWhiteEdges += 1;
      return pixel;
    });

    let transparentPixels = 0;
    let opaquePixels = 0;
    let opaqueWhitePixels = 0;
    let minOpaqueX = canvas.width;
    let minOpaqueY = canvas.height;
    let maxOpaqueX = -1;
    let maxOpaqueY = -1;
    for (let y = 0; y < canvas.height; y += 1) {
      for (let x = 0; x < canvas.width; x += 1) {
        const index = (y * canvas.width + x) * 4;
        const r = imageData[index];
        const g = imageData[index + 1];
        const b = imageData[index + 2];
        const a = imageData[index + 3];
        if (a === 0) transparentPixels += 1;
        if (a > 0) {
          opaquePixels += 1;
          minOpaqueX = Math.min(minOpaqueX, x);
          minOpaqueY = Math.min(minOpaqueY, y);
          maxOpaqueX = Math.max(maxOpaqueX, x);
          maxOpaqueY = Math.max(maxOpaqueY, y);
        }
        if (a > 240 && r > 245 && g > 245 && b > 245) opaqueWhitePixels += 1;
      }
    }

    let gridSamples = 0;
    let gridTransparent = 0;
    let gridOpaqueWhite = 0;
    for (let y = 0; y < canvas.height; y += Math.max(1, Math.floor(canvas.height / 18))) {
      for (let x = 0; x < canvas.width; x += Math.max(1, Math.floor(canvas.width / 32))) {
        const pixel = readPixel(x, y);
        gridSamples += 1;
        if (pixel.a === 0) gridTransparent += 1;
        if (pixel.a > 240 && pixel.r > 245 && pixel.g > 245 && pixel.b > 245) gridOpaqueWhite += 1;
      }
    }

    return {
      src,
      naturalWidth: image.naturalWidth,
      naturalHeight: image.naturalHeight,
      totalPixels: canvas.width * canvas.height,
      transparentPixels,
      opaquePixels,
      opaqueWhitePixels,
      opaqueBounds:
        opaquePixels > 0
          ? { minX: minOpaqueX, minY: minOpaqueY, maxX: maxOpaqueX, maxY: maxOpaqueY }
          : null,
      centerPixel: readPixel(Math.floor(canvas.width / 2), Math.floor(canvas.height / 2)),
      gridSamples,
      gridTransparent,
      gridOpaqueWhite,
      edgeSamples: samplePixels.length,
      transparentEdges,
      opaqueWhiteEdges,
      firstSamples: samplePixels.slice(0, 8)
    };
  }

  async function logRenderDiagnostics(reason: string) {
    const key = `${reason}:${currentMode}:${currentState}:${frameSrc}`;
    if (key === lastDiagnosticKey) return;
    lastDiagnosticKey = key;

    await tick();

    const media = mediaElement;
    const mediaInfo =
      media instanceof HTMLImageElement
        ? {
            tag: media.tagName,
            src: media.currentSrc || media.src,
            naturalWidth: media.naturalWidth,
            naturalHeight: media.naturalHeight,
            complete: media.complete
          }
        : media instanceof HTMLVideoElement
          ? {
              tag: media.tagName,
              src: media.currentSrc || media.src,
              videoWidth: media.videoWidth,
              videoHeight: media.videoHeight,
              readyState: media.readyState
            }
          : null;

    let frameAlpha: unknown = null;
    if (isFrameSequence) {
      try {
        frameAlpha = await inspectFrameAlpha(frameSrc);
      } catch (error) {
        frameAlpha = { error: String(error) };
      }
    }

    logDiagnostic('render-diagnostics', {
      reason,
      location: window.location.href,
      devicePixelRatio: window.devicePixelRatio,
      viewport: { width: window.innerWidth, height: window.innerHeight },
      styles: [
        computedStyleSummary('html'),
        computedStyleSummary('body'),
        computedStyleSummary('#app'),
        computedStyleSummary('.pet-shell'),
        computedStyleSummary('.pet-hit-area'),
        computedStyleSummary('.pet-media')
      ],
      media: mediaInfo,
      frameAlpha
    });
  }

  function stopFrameTimer() {
    if (frameTimer) clearInterval(frameTimer);
    frameTimer = undefined;
  }

  async function preloadFrameSequence(animation: AnimationItem, epoch: number) {
    if (!animation.framePattern || !animation.frameCount) return [];

    const urls = Array.from({ length: animation.frameCount }, (_, index) =>
      formatFrameSrc(animation.framePattern as string, index + 1)
    );

    await Promise.all(
      urls.map(async (src) => {
        const image = new Image();
        image.decoding = 'async';
        image.src = src;
        await image.decode();
      })
    );

    if (epoch !== preloadEpoch) return [];
    return urls;
  }

  async function startFrameTimer() {
    stopFrameTimer();
    const epoch = ++preloadEpoch;
    currentFrame = 0;
    activeFrameUrls = [];

    if (!currentAnimation.framePattern || !currentAnimation.frameCount) return;

    const urls = await preloadFrameSequence(currentAnimation, epoch);
    if (epoch !== preloadEpoch || urls.length === 0) return;
    activeFrameUrls = urls;
    logDiagnostic('frame-preload-complete', { frameCount: urls.length });

    const frameCount = currentAnimation.frameCount;
    const intervalMs = 1000 / (currentAnimation.fps ?? 10);
    frameTimer = setInterval(() => {
      currentFrame = (currentFrame + 1) % frameCount;
    }, intervalMs);
  }

  function animationTimerKey() {
    return [
      currentState,
      currentAnimation.src,
      currentAnimation.framePattern ?? '',
      currentAnimation.frameCount ?? 0,
      currentAnimation.fps ?? 0
    ].join(':');
  }

  function resetSleepTimer() {
    if (sleepTimer) clearTimeout(sleepTimer);
    const delayMs = currentMode === 'leisure' ? (config.leisureSleepAfterMs ?? 15_000) : config.sleepAfterMs;
    sleepTimer = setTimeout(() => {
      setPetState('sleep');
    }, delayMs);
  }

  function clearSleepTimer() {
    if (sleepTimer) clearTimeout(sleepTimer);
    sleepTimer = undefined;
  }

  function markActivity() {
    if (currentMode === 'interactive') return;
    if (currentMode === 'leisure') {
      clearSleepTimer();
      return;
    }
    if (currentState === 'sleep') {
      setPetState(config.defaultState);
    }
    resetSleepTimer();
  }

  function handlePointerEnter() {
    if (contextMenu) return;
    if (currentMode === 'interactive') return;
    if (currentMode === 'leisure') {
      clearSleepTimer();
      return;
    }
    markActivity();
    if (currentState !== 'click') setPetState('look');
  }

  function handlePointerMove(event: PointerEvent) {
    if (contextMenu) return;

    if (pointerDownAt && !isDragging) {
      const moved = Math.hypot(event.clientX - pointerDownAt.x, event.clientY - pointerDownAt.y);
      if (moved >= dragThreshold) {
        beginNativeDrag();
      }
    }

    if (currentMode !== 'interactive') markActivity();
  }

  function handlePointerLeave(event: PointerEvent) {
    if (contextMenu) return;
    if (currentMode === 'interactive') return;
    if (currentMode === 'leisure') {
      resetSleepTimer();
      return;
    }
    markActivity();
    if (currentState === 'click') return;

    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const dx = Math.max(bounds.left - event.clientX, 0, event.clientX - bounds.right);
    const dy = Math.max(bounds.top - event.clientY, 0, event.clientY - bounds.bottom);
    const isStillNear = Math.hypot(dx, dy) < pointerLookRadius;
    setPetState(isStillNear ? 'look' : config.defaultState);
  }

  function handlePointerDown(event: PointerEvent) {
    if (contextMenu) return;
    if (event.button !== 0) return;
    event.preventDefault();
    if (currentMode !== 'interactive') {
      markActivity();
    }
    pointerDownAt = { x: event.clientX, y: event.clientY };
    dragLocked = true;
    movementEpoch += 1;
    lastMoveAt = performance.now();
  }

  function handlePointerUp() {
    if (contextMenu) return;
    if (currentMode !== 'interactive') {
      markActivity();
    }
    pointerDownAt = null;
    if (!isDragging) {
      releaseDragLock();
    }
  }

  function handleClick(event: MouseEvent) {
    if (contextMenu) return;
    if (isDragging) return;
    if (currentMode === 'interactive') {
      const now = performance.now();
      const settings = interactionSettings();
      interactiveClicks = interactiveClicks.filter((time) => now - time <= settings.clickWindowMs);
      interactiveClicks.push(now);
      const count = interactiveClicks.length;
      if (count >= settings.heavyClickCount) {
        playInteractiveAction(['paw_back', 'fur_puff'], 1600);
        interactiveClicks = [];
      } else if (count >= settings.mediumClickCount) {
        playInteractiveAction(['meow'], 1300);
      } else if (count >= 2) {
        playInteractiveAction(['tail_annoyed'], 1000);
      } else {
        playInteractiveAction(['happy_squint', 'purr', 'nuzzle', 'belly'], 1200);
      }
      return;
    }
    if (currentMode === 'leisure') {
      if (leisureWakeClickTimer) clearTimeout(leisureWakeClickTimer);
      leisureWakeClickTimer = undefined;
      if (currentState !== 'sleep' || event.detail !== 1) return;

      leisureWakeClickTimer = setTimeout(() => {
        leisureWakeClickTimer = undefined;
        if (currentMode !== 'leisure' || currentState !== 'sleep') return;
        setPetState('click');
        clearSleepTimer();
      }, 220);
      return;
    }
    markActivity();
    setPetState('click');
  }

  function handleDoubleClick(event: MouseEvent) {
    if (currentMode !== 'leisure') return;
    event.preventDefault();
    event.stopPropagation();
    if (leisureWakeClickTimer) clearTimeout(leisureWakeClickTimer);
    leisureWakeClickTimer = undefined;
    if (currentState === 'click') {
      setPetState('sleep');
      clearSleepTimer();
    }
  }

  function handleWheel(event: WheelEvent) {
    if (contextMenu) return;
    event.preventDefault();
    const direction = event.deltaY < 0 ? 1 : -1;
    petScale = Math.min(maxPetScale, Math.max(minPetScale, petScale + direction * scaleStep));
    const targetScale = petScale;
    const session = ++resizeSession;
    if (resizeCenterTimer) clearTimeout(resizeCenterTimer);
    resizeQueue = resizeQueue.then(() => resizeWindowToScale(targetScale)).finally(() => {
      if (session !== resizeSession) return;
      resizeCenterTimer = setTimeout(() => {
        if (session === resizeSession) resizeCenter = null;
        resizeCenterTimer = undefined;
      }, 250);
    });
    logDiagnostic('pet-scale-changed', { scale: petScale });
  }

  async function resizeWindowToScale(targetScale: number) {
    if (!baseWindowSize) {
      const size = await appWindow.outerSize();
      baseWindowSize = { width: size.width, height: size.height };
    }

    try {
      if (!resizeCenter) {
        const [position, currentSize] = await Promise.all([appWindow.outerPosition(), appWindow.outerSize()]);
        resizeCenter = {
          x: position.x + currentSize.width / 2,
          y: position.y + currentSize.height / 2
        };
      }
      const width = Math.round(baseWindowSize.width * targetScale);
      const height = Math.round(baseWindowSize.height * targetScale);

      await appWindow.setSize(new PhysicalSize(width, height));
      await appWindow.setPosition(
        new PhysicalPosition(
          Math.round(resizeCenter.x - width / 2),
          Math.round(resizeCenter.y - height / 2)
        )
      );
      await syncWalkPositionToWindow();
    } catch (error) {
      logDiagnostic('pet-scale-resize-failed', { scale: targetScale, error: String(error) });
    }
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    pointerDownAt = null;

    const x = Math.min(event.clientX, window.innerWidth - contextMenuWidth);
    const y = Math.min(event.clientY, window.innerHeight - contextMenuHeight);
    contextMenu = {
      x: Math.max(4, x),
      y: Math.max(4, y)
    };
  }

  async function chooseContextMode(mode: PetMode) {
    closeContextMenu();
    await setPetMode(mode);
  }

  function beginNativeDrag() {
    if (isDragging) return;
    closeContextMenu();
    resizeCenter = null;
    resizeSession += 1;
    if (resizeCenterTimer) clearTimeout(resizeCenterTimer);
    resizeCenterTimer = undefined;
    isDragging = true;
    movementEpoch += 1;
    scheduleDragRelease(800);

    void appWindow
      .startDragging()
      .then(() => syncWalkPositionToWindow())
      .then(() => scheduleDragRelease(120))
      .catch(() => {
        isDragging = false;
        releaseDragLock();
      });
  }

  function scheduleDragRelease(delayMs = 180) {
    if (dragReleaseTimer) clearTimeout(dragReleaseTimer);
    dragReleaseTimer = setTimeout(() => {
      void syncWalkPositionToWindow().finally(() => {
        isDragging = false;
        releaseDragLock();
      });
    }, delayMs);
  }

  function releaseDragLock() {
    if (dragReleaseTimer) clearTimeout(dragReleaseTimer);
    dragReleaseTimer = undefined;
    pointerDownAt = null;
    dragLocked = false;
    movementEpoch += 1;
    lastMoveAt = performance.now();
  }

  async function getMovementBounds() {
    const scaleFactor = window.devicePixelRatio || 1;
    const screen = window.screen as Screen & { availLeft?: number; availTop?: number };
    const screenMinX = (screen.availLeft ?? 0) * scaleFactor;
    const screenMinY = (screen.availTop ?? 0) * scaleFactor;
    const screenWidth = screen.availWidth * scaleFactor;
    const screenHeight = screen.availHeight * scaleFactor;
    const windowWidth = window.innerWidth * scaleFactor;
    const windowHeight = window.innerHeight * scaleFactor;

    if (screenWidth > 0 && screenHeight > 0 && windowWidth > 0 && windowHeight > 0) {
      const minX = screenMinX;
      const maxX = screenMinX + screenWidth - windowWidth;
      const minY = screenMinY;
      const maxY = screenMinY + screenHeight - windowHeight;
      const bottomY = screenMinY + screenHeight - windowHeight - config.movement.bottomMarginPx * scaleFactor;

      return {
        minX,
        maxX: Math.max(minX, maxX),
        minY,
        maxY: Math.max(minY, maxY),
        bottomY: Math.min(Math.max(bottomY, minY), Math.max(minY, maxY))
      };
    }

    const monitor = (await currentMonitor()) ?? (await primaryMonitor());
    if (!monitor) return null;

    const size = await appWindow.outerSize();
    const workArea = monitor.workArea;
    const minX = workArea.position.x;
    const maxX = workArea.position.x + workArea.size.width - size.width;
    const minY = workArea.position.y;
    const maxY = workArea.position.y + workArea.size.height - size.height;
    const bottomY = workArea.position.y + workArea.size.height - size.height - config.movement.bottomMarginPx;

    return {
      minX,
      maxX: Math.max(minX, maxX),
      minY,
      maxY: Math.max(minY, maxY),
      bottomY: Math.min(Math.max(bottomY, minY), Math.max(minY, maxY))
    };
  }

  async function syncWalkPositionToWindow() {
    const position = await appWindow.outerPosition();
    walkX = position.x;
    walkY = position.y;
  }

  async function moveAlongDesktopBottom() {
    if (!canAutoWalk) {
      lastMoveAt = performance.now();
      return;
    }

    if (isPositioning) return;
    isPositioning = true;
    const epoch = movementEpoch;

    try {
      const bounds = await getMovementBounds();
      if (!bounds) return;
      if (!canAutoWalk || epoch !== movementEpoch) return;

      if (walkX === undefined) {
        const position = await appWindow.outerPosition();
        if (!canAutoWalk || epoch !== movementEpoch) return;
        walkX = Math.min(bounds.maxX, Math.max(bounds.minX, position.x));
        walkY = Math.min(bounds.maxY, Math.max(bounds.minY, position.y));
      }

      const now = performance.now();
      const elapsedSeconds = Math.min((now - lastMoveAt) / 1000, 0.2);
      lastMoveAt = now;

      walkX += walkDirection * config.movement.speedPxPerSecond * elapsedSeconds;
      if (walkX <= bounds.minX) {
        walkX = bounds.minX;
        walkDirection = 1;
      } else if (walkX >= bounds.maxX) {
        walkX = bounds.maxX;
        walkDirection = -1;
      }

      const y = Math.min(bounds.maxY, Math.max(bounds.minY, walkY ?? bounds.bottomY));
      walkY = y;
      if (!canAutoWalk || epoch !== movementEpoch) return;
      await appWindow.setPosition(new PhysicalPosition(Math.round(walkX), Math.round(y)));
    } finally {
      isPositioning = false;
    }
  }

  async function setPetMode(nextMode: PetMode) {
    currentMode = nextMode;
    pointerDownAt = null;
    isDragging = false;
    dragLocked = false;
    movementEpoch += 1;

    if (clickTimer) clearTimeout(clickTimer);
    if (sleepTimer) clearTimeout(sleepTimer);
    if (interactionResetTimer) clearTimeout(interactionResetTimer);
    if (interactionHoverTimer) clearTimeout(interactionHoverTimer);
    interactionActionUntil = 0;
    interactionPhase = 'away';
    interactionOffsetX = 0;
    interactionOffsetY = 0;
    interactiveClicks = [];
    lastCursorSample = null;

    if (nextMode === 'interactive') {
      setPetState('play');
      await syncWalkPositionToWindow();
      void sampleInteractivePointer();
      return;
    }

    if (nextMode === 'leisure') {
      setPetState('click');
      clearSleepTimer();
      await syncWalkPositionToWindow();
      return;
    }

    setPetState(config.defaultState);
    resetSleepTimer();

    if (nextMode === 'patrol') {
      const bounds = await getMovementBounds();
      if (bounds) {
        const position = await appWindow.outerPosition();
        walkX = Math.min(bounds.maxX, Math.max(bounds.minX, position.x));
        walkY = bounds.bottomY;
        await appWindow.setPosition(new PhysicalPosition(Math.round(walkX), Math.round(walkY)));
      } else {
        await syncWalkPositionToWindow();
      }
      return;
    }

    await syncWalkPositionToWindow();
  }

  function isPetMode(value: string): value is PetMode {
    return value === 'leisure' || value === 'patrol' || value === 'interactive';
  }

  $effect(() => {
    const key = animationTimerKey();
    if (key === lastAnimationKey) return;
    lastAnimationKey = key;
    void startFrameTimer();
    setTimeout(() => {
      void logRenderDiagnostics('animation-changed');
    }, 0);
  });

  onMount(() => {
    let disposed = false;

    async function loadConfig() {
      try {
        const response = await fetch('/config/animations.json', { cache: 'no-store' });
        if (!response.ok) throw new Error(`Failed to load animation config: ${response.status}`);
        const loaded = (await response.json()) as AnimationConfig;
        if (disposed) return;
        config = loaded;
        logDiagnostic('config-loaded', { config: loaded });
        currentMode = loaded.defaultMode ?? fallbackConfig.defaultMode;
        currentState = loaded.defaultState;
        await setPetMode(currentMode);
      } catch (error) {
        console.error(error);
        logDiagnostic('config-load-failed', { error: String(error) });
        await setPetMode(currentMode);
      }
    }

    let unlistenMode: UnlistenFn | undefined;
    let unlistenMoved: UnlistenFn | undefined;

    void appWindow.setAlwaysOnTop(true);
    void appWindow.outerSize().then((size) => {
      baseWindowSize = { width: size.width, height: size.height };
    });
    logDiagnostic('app-mounted');
    void loadConfig();
    void listen<string>('pet://mode', (event) => {
      if (isPetMode(event.payload)) {
        closeContextMenu();
        void setPetMode(event.payload);
      }
    }).then((unlisten) => {
      unlistenMode = unlisten;
    });
    void appWindow.onMoved((event) => {
      if (!dragLocked && !isDragging) return;
      walkX = event.payload.x;
      walkY = event.payload.y;
      scheduleDragRelease(800);
    }).then((unlisten) => {
      unlistenMoved = unlisten;
    });
    movementTimer = setInterval(() => {
      void moveAlongDesktopBottom();
    }, config.movement.updateMs);
    interactionTimer = setInterval(() => {
      void sampleInteractivePointer().catch((error) => {
        logDiagnostic('interactive-pointer-error', { error: String(error) });
      });
    }, interactionSettings().pollMs);

    return () => {
      disposed = true;
      if (clickTimer) clearTimeout(clickTimer);
      if (sleepTimer) clearTimeout(sleepTimer);
      if (leisureWakeClickTimer) clearTimeout(leisureWakeClickTimer);
      if (movementTimer) clearInterval(movementTimer);
      if (dragReleaseTimer) clearTimeout(dragReleaseTimer);
      if (interactionTimer) clearInterval(interactionTimer);
      if (interactionResetTimer) clearTimeout(interactionResetTimer);
      if (interactionHoverTimer) clearTimeout(interactionHoverTimer);
      stopFrameTimer();
      if (unlistenMode) unlistenMode();
      if (unlistenMoved) unlistenMoved();
    };
  });
</script>

<main class="pet-shell" aria-label="Desktop pet">
  <button
    class="pet-hit-area"
    class:interactive-mode={currentMode === 'interactive'}
    aria-label="Juboss desktop pet"
    style={`--pet-scale: ${petScale}; --track-x: ${interactionOffsetX}px; --track-y: ${interactionOffsetY}px;`}
    oncontextmenu={handleContextMenu}
    onpointerenter={handlePointerEnter}
    onpointermove={handlePointerMove}
    onpointerleave={handlePointerLeave}
    onpointerdown={handlePointerDown}
    onpointerup={handlePointerUp}
    onclick={handleClick}
    ondblclick={handleDoubleClick}
    onwheel={handleWheel}
  >
    {#if isFrameSequence}
      <img
        bind:this={mediaElement}
        class="pet-media"
        src={frameSrc}
        alt={currentAnimation.alt ?? currentState}
        draggable="false"
        onerror={(event) => logDiagnostic('frame-load-error', { src: frameSrc, error: String(event) })}
      />
    {:else if isVideo}
      <video
        bind:this={mediaElement}
        class="pet-media"
        src={currentAnimation.src}
        aria-label={currentAnimation.alt ?? currentState}
        autoplay
        muted
        playsinline
        loop={currentAnimation.loop ?? true}
        onloadeddata={() => logRenderDiagnostics('video-loaded')}
      ></video>
    {:else}
      <img
        bind:this={mediaElement}
        class="pet-media"
        src={currentAnimation.src}
        alt={currentAnimation.alt ?? currentState}
        draggable="false"
        onload={() => logRenderDiagnostics('image-loaded')}
      />
    {/if}
  </button>

  {#if contextMenu}
    <div class="context-scrim" role="presentation" onclick={closeContextMenu} oncontextmenu={closeContextMenu}></div>
    <div
      class="pet-context-menu"
      role="menu"
      aria-label="模式选择"
      style={`left: ${contextMenu.x}px; top: ${contextMenu.y}px;`}
    >
      {#each Object.entries(modeLabels) as [mode, label]}
        <button
          class:active-mode={currentMode === mode}
          class="context-menu-item"
          role="menuitemradio"
          aria-checked={currentMode === mode}
          onclick={() => chooseContextMode(mode as PetMode)}
        >
          <span class="mode-check">{currentMode === mode ? '✓' : ''}</span>
          <span>{label}</span>
        </button>
      {/each}
    </div>
  {/if}
</main>
