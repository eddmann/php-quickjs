// Minimal MessagePack codec for the php-quickjs __host bridge.
//
// Covers exactly the types crossing the boundary: null, bool, int, float64,
// str (utf-8), bin (Uint8Array), array, and string-keyed map (plain object).
// It must stay byte-compatible with the Rust `MiddleValue` serde impl
// (src/marshal.rs), which reads/writes *native* msgpack types.
//
// Installed once per realm as globalThis.__mp = { encode, decode }.
if (!globalThis.__mp) (function () {
  "use strict";

  // QuickJS has no TextEncoder/TextDecoder (those are WHATWG, not ES), so
  // UTF-8 is handled manually here.
  function utf8Encode(str) {
    var bytes = [];
    for (var i = 0; i < str.length; i++) {
      var c = str.charCodeAt(i);
      if (c < 0x80) {
        bytes.push(c);
      } else if (c < 0x800) {
        bytes.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
      } else if (c >= 0xd800 && c <= 0xdbff) {
        // High surrogate; combine with the following low surrogate.
        var c2 = str.charCodeAt(++i);
        var cp = 0x10000 + ((c & 0x3ff) << 10) + (c2 & 0x3ff);
        bytes.push(
          0xf0 | (cp >> 18),
          0x80 | ((cp >> 12) & 0x3f),
          0x80 | ((cp >> 6) & 0x3f),
          0x80 | (cp & 0x3f)
        );
      } else {
        bytes.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      }
    }
    return bytes;
  }

  function utf8Decode(bytes, start, len) {
    var out = "";
    var i = start;
    var end = start + len;
    while (i < end) {
      var c = bytes[i++];
      if (c < 0x80) {
        out += String.fromCharCode(c);
      } else if (c < 0xe0) {
        out += String.fromCharCode(((c & 0x1f) << 6) | (bytes[i++] & 0x3f));
      } else if (c < 0xf0) {
        var b1 = bytes[i++];
        var b2 = bytes[i++];
        out += String.fromCharCode(
          ((c & 0x0f) << 12) | ((b1 & 0x3f) << 6) | (b2 & 0x3f)
        );
      } else {
        var d1 = bytes[i++];
        var d2 = bytes[i++];
        var d3 = bytes[i++];
        var cp =
          ((c & 0x07) << 18) |
          ((d1 & 0x3f) << 12) |
          ((d2 & 0x3f) << 6) |
          (d3 & 0x3f);
        cp -= 0x10000;
        out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
      }
    }
    return out;
  }

  // ---- encoder ----------------------------------------------------------
  function Writer() {
    this.bytes = [];
  }
  Writer.prototype.u8 = function (b) {
    this.bytes.push(b & 0xff);
  };
  Writer.prototype.u16 = function (n) {
    this.u8(n >> 8);
    this.u8(n);
  };
  Writer.prototype.u32 = function (n) {
    this.u8(n >> 24);
    this.u8(n >> 16);
    this.u8(n >> 8);
    this.u8(n);
  };
  Writer.prototype.raw = function (arr) {
    for (var i = 0; i < arr.length; i++) this.bytes.push(arr[i] & 0xff);
  };

  function encodeInt(w, n) {
    if (n >= 0) {
      if (n <= 0x7f) {
        w.u8(n);
      } else if (n <= 0xff) {
        w.u8(0xcc);
        w.u8(n);
      } else if (n <= 0xffff) {
        w.u8(0xcd);
        w.u16(n);
      } else if (n <= 0xffffffff) {
        w.u8(0xce);
        w.u32(n);
      } else {
        // uint64
        w.u8(0xcf);
        writeBig(w, n);
      }
    } else {
      if (n >= -32) {
        w.u8(0xe0 | (n + 32));
      } else if (n >= -128) {
        w.u8(0xd0);
        w.u8(n & 0xff);
      } else if (n >= -32768) {
        w.u8(0xd1);
        w.u16(n & 0xffff);
      } else if (n >= -2147483648) {
        w.u8(0xd2);
        w.u32(n >>> 0);
      } else {
        // int64
        w.u8(0xd3);
        writeBig(w, n);
      }
    }
  }

  // Write a 64-bit integer (best effort; exact up to 2^53).
  function writeBig(w, n) {
    var big = BigInt(n);
    if (big < 0n) big = (1n << 64n) + big;
    for (var i = 7; i >= 0; i--) {
      w.u8(Number((big >> BigInt(i * 8)) & 0xffn));
    }
  }

  function encodeValue(w, v) {
    if (v === null || v === undefined) {
      w.u8(0xc0);
    } else if (v === true) {
      w.u8(0xc3);
    } else if (v === false) {
      w.u8(0xc2);
    } else if (typeof v === "number") {
      if (Number.isInteger(v)) {
        encodeInt(w, v);
      } else {
        w.u8(0xcb);
        var buf = new ArrayBuffer(8);
        new DataView(buf).setFloat64(0, v, false);
        w.raw(new Uint8Array(buf));
      }
    } else if (typeof v === "bigint") {
      // Encode as int64/uint64.
      if (v >= 0n) w.u8(0xcf);
      else w.u8(0xd3);
      writeBig(w, v);
    } else if (typeof v === "string") {
      var enc = utf8Encode(v);
      var len = enc.length;
      if (len <= 31) {
        w.u8(0xa0 | len);
      } else if (len <= 0xff) {
        w.u8(0xd9);
        w.u8(len);
      } else if (len <= 0xffff) {
        w.u8(0xda);
        w.u16(len);
      } else {
        w.u8(0xdb);
        w.u32(len);
      }
      w.raw(enc);
    } else if (v instanceof Uint8Array) {
      var blen = v.length;
      if (blen <= 0xff) {
        w.u8(0xc4);
        w.u8(blen);
      } else if (blen <= 0xffff) {
        w.u8(0xc5);
        w.u16(blen);
      } else {
        w.u8(0xc6);
        w.u32(blen);
      }
      w.raw(v);
    } else if (Array.isArray(v)) {
      var alen = v.length;
      if (alen <= 15) {
        w.u8(0x90 | alen);
      } else if (alen <= 0xffff) {
        w.u8(0xdc);
        w.u16(alen);
      } else {
        w.u8(0xdd);
        w.u32(alen);
      }
      for (var i = 0; i < alen; i++) encodeValue(w, v[i]);
    } else if (typeof v === "object") {
      var keys = Object.keys(v);
      var mlen = keys.length;
      if (mlen <= 15) {
        w.u8(0x80 | mlen);
      } else if (mlen <= 0xffff) {
        w.u8(0xde);
        w.u16(mlen);
      } else {
        w.u8(0xdf);
        w.u32(mlen);
      }
      for (var k = 0; k < mlen; k++) {
        encodeValue(w, keys[k]);
        encodeValue(w, v[keys[k]]);
      }
    } else {
      throw new TypeError("cannot msgpack-encode value of type " + typeof v);
    }
  }

  function encode(v) {
    var w = new Writer();
    encodeValue(w, v);
    return new Uint8Array(w.bytes);
  }

  // ---- decoder ----------------------------------------------------------
  function Reader(bytes) {
    this.b = bytes;
    this.p = 0;
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }
  Reader.prototype.u8 = function () {
    return this.b[this.p++];
  };
  Reader.prototype.u16 = function () {
    var n = this.view.getUint16(this.p, false);
    this.p += 2;
    return n;
  };
  Reader.prototype.u32 = function () {
    var n = this.view.getUint32(this.p, false);
    this.p += 4;
    return n;
  };
  Reader.prototype.i8 = function () {
    var n = this.view.getInt8(this.p);
    this.p += 1;
    return n;
  };
  Reader.prototype.i16 = function () {
    var n = this.view.getInt16(this.p, false);
    this.p += 2;
    return n;
  };
  Reader.prototype.i32 = function () {
    var n = this.view.getInt32(this.p, false);
    this.p += 4;
    return n;
  };
  Reader.prototype.big = function (signed) {
    var hi = BigInt(this.u32());
    var lo = BigInt(this.u32());
    var n = (hi << 32n) | lo;
    if (signed && n >= 1n << 63n) n -= 1n << 64n;
    // Collapse to a Number when it is exactly representable.
    if (n >= -9007199254740991n && n <= 9007199254740991n) return Number(n);
    return n;
  };
  Reader.prototype.str = function (len) {
    var s = utf8Decode(this.b, this.p, len);
    this.p += len;
    return s;
  };
  Reader.prototype.bin = function (len) {
    var slice = this.b.slice(this.p, this.p + len);
    this.p += len;
    return slice;
  };

  function decodeValue(r) {
    var c = r.u8();
    if (c <= 0x7f) return c; // positive fixint
    if (c >= 0xe0) return c - 256; // negative fixint
    if (c >= 0x80 && c <= 0x8f) return decodeMap(r, c & 0x0f);
    if (c >= 0x90 && c <= 0x9f) return decodeArray(r, c & 0x0f);
    if (c >= 0xa0 && c <= 0xbf) return r.str(c & 0x1f);
    switch (c) {
      case 0xc0:
        return null;
      case 0xc2:
        return false;
      case 0xc3:
        return true;
      case 0xc4:
        return r.bin(r.u8());
      case 0xc5:
        return r.bin(r.u16());
      case 0xc6:
        return r.bin(r.u32());
      case 0xca: {
        var f = r.view.getFloat32(r.p, false);
        r.p += 4;
        return f;
      }
      case 0xcb: {
        var d = r.view.getFloat64(r.p, false);
        r.p += 8;
        return d;
      }
      case 0xcc:
        return r.u8();
      case 0xcd:
        return r.u16();
      case 0xce:
        return r.u32();
      case 0xcf:
        return r.big(false);
      case 0xd0:
        return r.i8();
      case 0xd1:
        return r.i16();
      case 0xd2:
        return r.i32();
      case 0xd3:
        return r.big(true);
      case 0xd9:
        return r.str(r.u8());
      case 0xda:
        return r.str(r.u16());
      case 0xdb:
        return r.str(r.u32());
      case 0xdc:
        return decodeArray(r, r.u16());
      case 0xdd:
        return decodeArray(r, r.u32());
      case 0xde:
        return decodeMap(r, r.u16());
      case 0xdf:
        return decodeMap(r, r.u32());
      default:
        throw new TypeError("unknown msgpack marker 0x" + c.toString(16));
    }
  }

  function decodeArray(r, len) {
    var out = new Array(len);
    for (var i = 0; i < len; i++) out[i] = decodeValue(r);
    return out;
  }

  function decodeMap(r, len) {
    var out = {};
    for (var i = 0; i < len; i++) {
      var k = decodeValue(r);
      out[k] = decodeValue(r);
    }
    return out;
  }

  function decode(bytes) {
    return decodeValue(new Reader(bytes));
  }

  globalThis.__mp = { encode: encode, decode: decode };
})();
