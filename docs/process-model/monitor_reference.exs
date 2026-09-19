# Independent OTP reference fixture. Run with the pinned comparison toolchain.
# Fixed output erases only opaque process/reference values, never event order.
parent = self()
worker = spawn(fn ->
  receive do
    :go ->
      send(parent, {:payload, 9_223_372_036_854_775_807})
      exit({:fault, 1})
  end
end)
first = Process.monitor(worker)
second = Process.monitor(worker)
IO.puts("distinct=#{first != second}")
send(worker, :go)
receive do
  {:payload, value} -> IO.puts("payload=#{value}")
after
  5_000 -> raise "payload missing"
end
receive do
  {:DOWN, ^first, :process, ^worker, {:fault, 1}} -> IO.puts("first=fault:1")
after
  5_000 -> raise "first DOWN missing"
end
# A selective ordinary-message receive leaves the other lifecycle event queued.
send(self(), :ordinary)
receive do
  :ordinary -> IO.puts("ordinary=selected")
after
  5_000 -> raise "ordinary message missing"
end
receive do
  {:DOWN, ^second, :process, ^worker, {:fault, 1}} -> IO.puts("second=fault:1")
after
  5_000 -> raise "second DOWN missing"
end
late = Process.monitor(worker)
receive do
  {:DOWN, ^late, :process, ^worker, :noproc} -> IO.puts("late=noproc")
after
  5_000 -> raise "dead-process DOWN missing"
end

cancelled_worker = spawn(fn -> receive do :go -> :ok end end)
cancelled = Process.monitor(cancelled_worker)
barrier = Process.monitor(cancelled_worker)
IO.puts("cancelled=#{Process.demonitor(cancelled, [:info])}")
send(cancelled_worker, :go)
receive do
  {:DOWN, ^barrier, :process, ^cancelled_worker, :normal} -> :ok
after
  5_000 -> raise "cancellation barrier missing"
end
receive do
  {:DOWN, ^cancelled, :process, _, _} -> raise "cancelled monitor delivered"
after
  0 -> IO.puts("cancelled_delivery=none")
end
IO.puts("consumed_info=#{Process.demonitor(barrier, [:info])}")
IO.puts("consumed_flush_info=#{Process.demonitor(barrier, [:flush, :info])}")

self_ref = Process.monitor(self())
IO.puts("self_info=#{Process.demonitor(self_ref, [:info])}")
active_worker = spawn(fn -> receive do :go -> :ok end end)
active_ref = Process.monitor(active_worker)
IO.puts("active_flush_info=#{Process.demonitor(active_ref, [:flush, :info])}")
IO.puts("repeated_flush_info=#{Process.demonitor(active_ref, [:flush, :info])}")
send(active_worker, :go)
