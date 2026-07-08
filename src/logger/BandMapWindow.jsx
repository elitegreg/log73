import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { BAND_MAP_ROW_HEIGHT_PX, bandMapRows } from '../domain/bandMap';

function contextMenuPositionStyle(contextMenu) {
  const menuWidth = 120;
  const menuHeight = 36;
  const viewportWidth =
    typeof window === 'undefined' ? menuWidth : window.innerWidth;
  const viewportHeight =
    typeof window === 'undefined' ? menuHeight : window.innerHeight;

  return {
    left: Math.max(0, Math.min(contextMenu.x, viewportWidth - menuWidth)),
    top: Math.max(0, Math.min(contextMenu.y, viewportHeight - menuHeight)),
  };
}

function BandMapWindow({
  spotStore,
  radioFrequencyHz,
  height = null,
  onSpotClick,
  onDeleteSpot,
}) {
  const scrollContainerRef = useRef(null);
  const userScrolledRef = useRef(false);
  const autoScrollingRef = useRef(false);
  const userScrollIntentRef = useRef(false);
  const previousVfoTenthKhzRef = useRef(null);
  const previousVfoIndexRef = useRef(null);
  const contextMenuOpenedAtRef = useRef(0);
  const [contextMenu, setContextMenu] = useState(null);
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

  useEffect(() => {
    if (!contextMenu) return undefined;

    function closeContextMenu(event) {
      if (
        event?.type === 'click' &&
        Date.now() - contextMenuOpenedAtRef.current < 250
      ) {
        return;
      }
      setContextMenu(null);
    }

    window.addEventListener('click', closeContextMenu);
    window.addEventListener('keydown', closeContextMenu);
    window.addEventListener('blur', closeContextMenu);
    return () => {
      window.removeEventListener('click', closeContextMenu);
      window.removeEventListener('keydown', closeContextMenu);
      window.removeEventListener('blur', closeContextMenu);
    };
  }, [contextMenu]);

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

  function handleRowContextMenu(event, spot) {
    if (!onDeleteSpot || !spot) return;
    event.preventDefault();
    event.stopPropagation();
    contextMenuOpenedAtRef.current = Date.now();
    setContextMenu({ x: event.clientX, y: event.clientY, spot });
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
                      isClickableSpot
                        ? (event) => {
                            if (event.button !== 0) return;
                            onSpotClick(row.spot);
                          }
                        : undefined
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
                    <td
                      className="band-map-vfo-marker"
                      onContextMenu={
                        row.type === 'spot'
                          ? (event) => handleRowContextMenu(event, row.spot)
                          : undefined
                      }
                    >
                      {row.marker}
                    </td>
                    <td
                      className="band-map-frequency"
                      onContextMenu={
                        row.type === 'spot'
                          ? (event) => handleRowContextMenu(event, row.spot)
                          : undefined
                      }
                    >
                      {row.khz}
                    </td>
                    <td
                      className="band-map-callsign"
                      onContextMenu={
                        row.type === 'spot'
                          ? (event) => handleRowContextMenu(event, row.spot)
                          : undefined
                      }
                    >
                      {row.callsign}
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
      {contextMenu && typeof document !== 'undefined'
        ? createPortal(
            <div
              role="menu"
              aria-label="Band map spot actions"
              className="band-map-context-menu"
              style={contextMenuPositionStyle(contextMenu)}
              onClick={(event) => event.stopPropagation()}
            >
              <button
                type="button"
                onClick={() => {
                  onDeleteSpot?.(contextMenu.spot);
                  setContextMenu(null);
                }}
              >
                Delete
              </button>
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}

export default BandMapWindow;
