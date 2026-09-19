# Independent OTP reference. Handshakes establish order; timeouts only fail tests.
defmodule LinkReference do
  def take(pattern) do
    receive do
      ^pattern -> :ok
    after
      5_000 -> raise "missing #{inspect(pattern)}"
    end
  end

  def down(ref, pid, reason), do: take({:DOWN, ref, :process, pid, reason})

  def absent(pattern) do
    receive do
      ^pattern -> raise "unexpected #{inspect(pattern)}"
    after
      0 -> :ok
    end
  end

  def waiting do
    receive do
      {:ping, parent} ->
        send(parent, {:pong, self()})
        waiting()
      {:finish, reason} -> exit(reason)
    end
  end

  def run do
    parent = self()
    IO.puts("trap_previous=#{Process.flag(:trap_exit, true)}")
    IO.puts("trap_previous_again=#{Process.flag(:trap_exit, true)}")

    # Both registrations describe one edge. DOWN is a death-ordering barrier.
    child = spawn(fn -> waiting() end)
    ref = Process.monitor(child)
    Process.link(child)
    Process.link(child)
    send(child, {:finish, {:fault, 7}})
    down(ref, child, {:fault, 7})
    take({:EXIT, child, {:fault, 7}})
    absent({:EXIT, child, {:fault, 7}})
    IO.puts("idempotent=fault:7:once")

    child = spawn_link(fn -> exit(:normal) end)
    take({:EXIT, child, :normal})
    IO.puts("atomic_spawn_link=normal")

    # A normal signal and following ping come from the same sender.
    {child, ref} = spawn_monitor(fn -> waiting() end)
    :erlang.exit_signal(child, :normal)
    send(child, {:ping, parent})
    take({:pong, child})
    IO.puts("untrapped_normal=ignored")
    :erlang.exit_signal(child, :kill)
    down(ref, child, :killed)

    {child, ref} = spawn_monitor(fn ->
      Process.flag(:trap_exit, true)
      send(parent, {:ready, self()})
      waiting()
    end)
    take({:ready, child})
    :erlang.exit_signal(child, :kill)
    down(ref, child, :killed)
    IO.puts("direct_kill=killed")

    # Local exit(:kill) sends an ordinary linked exit, which can be trapped.
    child = spawn_link(fn -> exit(:kill) end)
    take({:EXIT, child, :kill})
    IO.puts("linked_kill=trapped:kill")

    {observer, ref} = spawn_monitor(fn ->
      spawn_link(fn -> exit(:kill) end)
      waiting()
    end)
    down(ref, observer, :kill)
    IO.puts("linked_kill_untrapped=kill")

    child = spawn_link(fn -> waiting() end)
    ref = Process.monitor(child)
    Process.unlink(child)
    send(child, {:finish, :shutdown})
    down(ref, child, :shutdown)
    absent({:EXIT, child, :shutdown})
    IO.puts("unlink_before_death=none")

    child = spawn_link(fn -> waiting() end)
    ref = Process.monitor(child)
    send(child, {:finish, :shutdown})
    down(ref, child, :shutdown)
    Process.unlink(child)
    take({:EXIT, child, :shutdown})
    IO.puts("unlink_queued_exit=preserved")

    # unlink only concerns links, never explicit exit signals.
    {child, ref} = spawn_monitor(fn ->
      Process.flag(:trap_exit, true)
      send(parent, {:ready, self()})
      take({:EXIT, parent, {:failure, "explicit"}})
      send(parent, {:explicit, self()})
    end)
    take({:ready, child})
    Process.unlink(child)
    :erlang.exit_signal(child, {:failure, "explicit"})
    take({:explicit, child})
    down(ref, child, :normal)
    IO.puts("unlink_explicit_exit=preserved")

    {dead, ref} = spawn_monitor(fn -> :ok end)
    down(ref, dead, :normal)
    Process.link(dead)
    take({:EXIT, dead, :noproc})
    IO.puts("dead_link=noproc")
    true = :erlang.exit_signal(dead, :kill)
    IO.puts("dead_signal=ok")

    Process.link(self())
    Process.unlink(self())
    IO.puts("self_link=noop")
    {child, ref} = spawn_monitor(fn ->
      :erlang.exit_signal(self(), :normal)
      send(parent, {:self_normal, self()})
    end)
    take({:self_normal, child})
    down(ref, child, :normal)
    IO.puts("self_normal_signal=ignored")
  end
end

LinkReference.run()
