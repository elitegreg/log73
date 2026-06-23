import React, { useEffect, useMemo, useRef } from 'react';
import { BAND_MAP_ROW_HEIGHT_PX, bandMapRows } from '../domain/bandMap';

function BandMapWindow({
  spotStore,
  radioFrequencyHz,
  height = null,
  onSpotClick,
}) {
  const scrollContainerRef = useRef(null);
  const userScrolledRef = useRef(false);
  const autoScrollingRef = useRef(false);
  const userScrollIntentRef = useRef(false);
  const previousVfoTenthKhzRef = useRef(null);
  const previousVfoIndexRef = useRef(null);
  const rows = useMemo(
    () => bandMapRows(spotStore, radioFrequencyHz),
    [spotStore, radioFrequencyHz],
  );
  const vfoRow = rows.find((row) => row.isVfo);

  useEffect(() => {
    const container = scrollContainerRef.current;
    if (!container || !vfoRow) return;

    const rowIndex = rows.findIndex((row) => row.key === vfoRow.key);
    if (rowIndex < 0) return;

    const previousVfoTenthKhz = previousVfoTenthKhzRef.current;
    const previousVfoIndex = previousVfoIndexRef.current;
    previousVfoTenthKhzRef.current = vfoRow.frequencyTenthKhz;
    previousVfoIndexRef.current = rowIndex;

    if (userScrolledRef.current) return;

    const rowTop = rowIndex * BAND_MAP_ROW_HEIGHT_PX;
    const rowBottom = rowTop + BAND_MAP_ROW_HEIGHT_PX;
    const vfoFrequencyChanged =
      previousVfoTenthKhz !== null &&
      previousVfoTenthKhz !== vfoRow.frequencyTenthKhz;
    const vfoRowShift =
      previousVfoIndex === null ? 0 : rowIndex - previousVfoIndex;

    if (!vfoFrequencyChanged && vfoRowShift !== 0) {
      setProgrammaticScroll(
        container,
        container.scrollTop + vfoRowShift * BAND_MAP_ROW_HEIGHT_PX,
      );
      return;
    }

    const middleHalfTop = container.scrollTop + container.clientHeight * 0.25;
    const middleHalfBottom =
      container.scrollTop + container.clientHeight * 0.75;
    if (rowTop >= middleHalfTop && rowBottom <= middleHalfBottom) return;

    setProgrammaticScroll(
      container,
      rowTop - Math.floor(container.clientHeight / 2),
    );
  }, [rows, vfoRow]);

  function setProgrammaticScroll(container, requestedScrollTop) {
    const maxScrollTop = Math.max(
      0,
      container.scrollHeight - container.clientHeight,
    );
    const nextScrollTop = Math.max(
      0,
      Math.min(requestedScrollTop, maxScrollTop),
    );
    if (Math.abs(container.scrollTop - nextScrollTop) < 1) return;

    autoScrollingRef.current = true;
    container.scrollTop = nextScrollTop;
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        autoScrollingRef.current = false;
      });
    });
  }

  function markUserScrollIntent() {
    userScrollIntentRef.current = true;
  }

  function handleScroll() {
    if (autoScrollingRef.current) return;
    if (!userScrollIntentRef.current) return;
    userScrolledRef.current = true;
  }

  return (
    <div
      className="band-map-window"
      aria-label="Band map spots"
      style={height ? { height: `${height}px` } : undefined}
    >
      <div className="band-map-title-bar">Band Map</div>
      <div
        className="band-map-table-scroll"
        ref={scrollContainerRef}
        onScroll={handleScroll}
        onWheel={markUserScrollIntent}
        onPointerDown={markUserScrollIntent}
        onTouchStart={markUserScrollIntent}
      >
        <table className="band-map-table">
          <colgroup>
            <col className="band-map-vfo-col" />
            <col className="band-map-frequency-col" />
            <col className="band-map-callsign-col" />
          </colgroup>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td className="band-map-empty" colSpan={3}>
                  No spots.
                </td>
              </tr>
            ) : (
              rows.map((row) => {
                const isClickableSpot = row.type === 'spot' && onSpotClick;
                return (
                  <tr
                    key={row.key}
                    className={[
                      row.isVfo ? 'band-map-vfo-row' : '',
                      isClickableSpot ? 'band-map-spot-row' : '',
                    ]
                      .filter(Boolean)
                      .join(' ')}
                    onClick={
                      isClickableSpot ? () => onSpotClick(row.spot) : undefined
                    }
                    onKeyDown={
                      isClickableSpot
                        ? (event) => {
                            if (event.key !== 'Enter' && event.key !== ' ')
                              return;
                            event.preventDefault();
                            onSpotClick(row.spot);
                          }
                        : undefined
                    }
                    tabIndex={isClickableSpot ? 0 : undefined}
                  >
                    <td className="band-map-vfo-marker">{row.marker}</td>
                    <td className="band-map-frequency">{row.khz}</td>
                    <td className="band-map-callsign">{row.callsign}</td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export default BandMapWindow;
