/**
 * live2d-adapter.js
 * Bridges the Cubism 4 SDK (PIXI v4 + LIVE2DCUBISMFRAMEWORK) to app.js.
 * No jQuery required.  Exposes window.live2dAdapter.
 *
 * Load order in index.html must be:
 *   1. pixi.min.js
 *   2. live2dcubismcore.min.js
 *   3. live2dcubismframework.js
 *   4. live2dcubismpixi.js
 *   5. l2d.js
 *   6. live2d-adapter.js   ← this file
 */
(function (global) {
  'use strict';

  // ── Canvas dimensions (must match the <canvas> element) ───────────────────
  const CANVAS_W = 280;
  const CANVAS_H = 560;

  let _app       = null;   // PIXI.Application
  let _model     = null;   // LIVE2DCUBISMPIXI.Model
  let _readyCb   = null;   // callback fired once model + idle are ready

  // ── Custom per-frame update (replaces model.update) ───────────────────────
  // Called with `this` = the model object.
  function _cubismUpdate(delta) {
    const dt = 0.016 * delta;

    // Auto-restart idle when no motion is playing
    if (!this.animator.isPlaying) {
      const m = this.motions.get('idle');
      if (m) this.animator.getLayer('base').play(m);
    }

    this._animator.updateAndEvaluate(dt);

    if (this.inDrag) {
      this.addParameterValueById('ParamAngleX',      this.pointerX *  30);
      this.addParameterValueById('ParamAngleY',     -this.pointerY *  30);
      this.addParameterValueById('ParamBodyAngleX',  this.pointerX *  10);
      this.addParameterValueById('ParamBodyAngleY', -this.pointerY *  10);
      this.addParameterValueById('ParamEyeBallX',    this.pointerX);
      this.addParameterValueById('ParamEyeBallY',   -this.pointerY);
    }

    if (this._physicsRig) this._physicsRig.updateAndEvaluate(dt);
    this._coreModel.update();

    let sort = false;
    for (let i = 0; i < this._meshes.length; i++) {
      const flags = this._coreModel.drawables.dynamicFlags[i];
      this._meshes[i].alpha   = this._coreModel.drawables.opacities[i];
      this._meshes[i].visible = Live2DCubismCore.Utils.hasIsVisibleBit(flags);
      if (Live2DCubismCore.Utils.hasVertexPositionsDidChangeBit(flags)) {
        this._meshes[i].vertices    = this._coreModel.drawables.vertexPositions[i];
        this._meshes[i].dirtyVertex = true;
      }
      if (Live2DCubismCore.Utils.hasRenderOrderDidChangeBit(flags)) sort = true;
    }
    if (sort) {
      this.children.sort((a, b) => {
        const ai = this._meshes.indexOf(a);
        const bi = this._meshes.indexOf(b);
        return this._coreModel.drawables.renderOrders[ai]
             - this._coreModel.drawables.renderOrders[bi];
      });
    }
    this._coreModel.drawables.resetDynamicFlags();
  }

  // ── Fit model to fill the canvas, then center ─────────────────────────────
  function _fitAndCenter() {
    if (!_model) return;

    // Heuristic initial scale based on demo ratios:
    // The demo (1280×720) uses scale = pos.x * 0.06 = 640*0.06 = 38.4 and fills
    // the screen height (~720 px).  Scaling to our 560 px canvas height:
    //   s = 38.4 * (CANVAS_H / 720) ≈ 29.9
    let s = CANVAS_H * 0.053;
    _model.scale = new PIXI.Point(s, s);
    _model.position.set(CANVAS_W / 2, CANVAS_H / 2);
    _model.masks.resize(CANVAS_W, CANVAS_H);

    // Read actual bounding box and constrain if wider than canvas
    const mw = _model.width;
    const mh = _model.height;
    if (mw > 0 && mh > 0) {
      const byW = (CANVAS_W * 0.96) / mw;
      const byH = (CANVAS_H * 0.96) / mh;
      const fit = Math.min(byW, byH);
      if (fit < 1) {
        s *= fit;
        _model.scale = new PIXI.Point(s, s);
      }
    }
    // Re-center after potential scale change
    _model.position.set(CANVAS_W / 2, CANVAS_H / 2);
    _model.masks.resize(CANVAS_W, CANVAS_H);
  }

  // ── Called by L2D once the model finishes loading ─────────────────────────
  function _onModelLoaded(model) {
    _model = model;
    _model.update = _cubismUpdate;
    _model.animator.addLayer(
      'base',
      LIVE2DCUBISMFRAMEWORK.BuiltinAnimationBlenders.OVERRIDE,
      1
    );

    _app.stage.addChild(_model);
    _app.stage.addChild(_model.masks);

    // Let PIXI render one frame so bounding-box data is available, then fit
    setTimeout(_fitAndCenter, 80);

    // Play idle and fire the ready callback
    setTimeout(() => {
      if (_model) {
        const m = _model.motions.get('idle');
        if (m) _model.animator.getLayer('base').play(m);
      }
      if (_readyCb) _readyCb();
    }, 120);
  }

  // ── Public API ────────────────────────────────────────────────────────────

  /**
   * Boot PIXI and load a Cubism 4 model.
   *
   * @param {HTMLCanvasElement} canvasEl    The <canvas> to render into.
   * @param {string}            basePath   Base URL for assets, e.g. 'assets'.
   * @param {string}            modelName  Subdir + stem, e.g. 'dujiaoshou_4'.
   * @param {Function}          [onReady]  Called when the model is loaded and idle.
   */
  function init(canvasEl, basePath, modelName, onReady) {
    _readyCb = onReady || null;

    _app = new PIXI.Application(CANVAS_W, CANVAS_H, {
      view:        canvasEl,
      transparent: true,
      antialias:   true,
    });

    _app.ticker.add((delta) => {
      if (!_model) return;
      _model.update(delta);
      _model.masks.update(_app.renderer);
    });

    // Wire up mouse drag on the canvas so the model follows the pointer
    const view = canvasEl;
    let isClick = false;
    view.addEventListener('mousedown', () => { isClick = true; });
    view.addEventListener('mousemove', (e) => {
      if (isClick) { isClick = false; if (_model) _model.inDrag = true; }
      if (_model) {
        const mx = _model.position.x - e.offsetX;
        const my = _model.position.y - e.offsetY;
        _model.pointerX = -mx / view.height;
        _model.pointerY = -my / view.width;
      }
    });
    view.addEventListener('mouseup', () => {
      isClick = false;
      if (_model) _model.inDrag = false;
    });

    const l2d = new L2D(basePath);
    l2d.load(modelName, { changeCanvas: _onModelLoaded });
  }

  /**
   * Play a named motion, e.g. 'idle', 'touch_head', 'complete'.
   * Silently falls back to 'idle' when the name isn't found.
   */
  function startMotion(name) {
    if (!_model) return;
    const m = _model.motions.get(name) || _model.motions.get('idle');
    if (!m) return;
    const layer = _model.animator.getLayer('base');
    if (layer) layer.play(m);
  }

  /** Returns all loaded motion names (useful for debugging). */
  function listMotions() {
    return _model ? [..._model.motions.keys()] : [];
  }

  Object.defineProperty(global, 'live2dAdapter', {
    value: { init, startMotion, listMotions },
    writable: false,
    configurable: false,
  });

})(window);
