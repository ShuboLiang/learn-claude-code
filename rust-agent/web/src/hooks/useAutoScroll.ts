import { useEffect, useRef, useState } from 'react'

/**
 * Auto-scroll to bottom when content changes, unless the user has scrolled up
 * more than `threshold` pixels from the bottom.
 */
export function useAutoScroll(
  deps: unknown[],
  threshold = 80,
): [React.RefObject<HTMLDivElement | null>, boolean] {
  const ref = useRef<HTMLDivElement | null>(null)
  const [isAtBottom, setIsAtBottom] = useState(true)

  // Track whether user is at the bottom
  useEffect(() => {
    const el = ref.current
    if (!el) return

    const handleScroll = () => {
      const { scrollTop, scrollHeight, clientHeight } = el
      setIsAtBottom(scrollHeight - scrollTop - clientHeight <= threshold)
    }

    el.addEventListener('scroll', handleScroll, { passive: true })
    return () => el.removeEventListener('scroll', handleScroll)
  }, [threshold])

  // Auto-scroll when deps change and user is at bottom
  useEffect(() => {
    const el = ref.current
    if (!el || !isAtBottom) return
    el.scrollTop = el.scrollHeight
  }, deps)

  return [ref, isAtBottom]
}
