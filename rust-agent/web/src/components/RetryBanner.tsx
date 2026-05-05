import { useEffect, useState } from 'react'
import { Clock } from 'lucide-react'

interface Props {
  attempt: number
  maxRetries: number
  waitSeconds: number
  detail?: string
}

export function RetryBanner({ attempt, maxRetries, waitSeconds, detail }: Props) {
  const [remaining, setRemaining] = useState(waitSeconds)

  useEffect(() => {
    setRemaining(waitSeconds)
    const interval = setInterval(() => {
      setRemaining((r) => Math.max(0, r - 1))
    }, 1000)
    return () => clearInterval(interval)
  }, [waitSeconds, attempt])

  return (
    <div className="flex items-center gap-2 rounded border border-yellow-500/40 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-600 dark:text-yellow-400">
      <Clock className="h-3.5 w-3.5 shrink-0" />
      <span>
        Retry {attempt}/{maxRetries}
        {remaining > 0 && <> — waiting {remaining}s</>}
      </span>
      {detail && (
        <span className="text-muted-foreground">({detail})</span>
      )}
    </div>
  )
}
