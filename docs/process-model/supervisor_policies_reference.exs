# Supplemental reference — 2026-09-19. Validation evidence is recorded separately.
# Pin OTP 29.0.6 / Elixir 1.20.4 before recording any accepted reference evidence.
# Watchdogs bound failure only; no elapsed time or scheduler ordering is an oracle.
defmodule SupervisorSupplement.Worker do
  def start_link({name, log, owner, mode}) do
    mode = case mode do
      {:script, script} -> Agent.get_and_update(script, fn
        [head] = all -> {head, all}
        [head | tail] -> {head, tail}
      end)
      mode -> mode
    end
    case mode do
      :ignore ->
        record(log, "ignore:#{name}")
        :ignore
      {:fail, reason} ->
        record(log, "fail:#{name}")
        {:error, reason}
      mode ->
        supervisor = self()
        pid = spawn_link(fn ->
          Process.flag(:trap_exit, true)
          record(log, "start:#{name}")
          send(supervisor, {:ready, self()})
          loop(name, log, owner, supervisor, mode)
        end)
        receive do
          {:ready, ^pid} -> {:ok, pid}
        after
          5_000 -> raise "child acknowledgement missing"
        end
    end
  end

  def record(log, text), do: Agent.update(log, &(&1 ++ [text]))
  def loop(name, log, owner, supervisor, mode) do
    receive do
      {:finish, reason} -> exit(reason)
      {:unlink, caller} ->
        Process.unlink(supervisor)
        send(caller, {:unlinked, self()})
        loop(name, log, owner, supervisor, mode)
      {:EXIT, _, reason} ->
        record(log, "stop:#{name}")
        if mode == :hold do
          send(owner, {:shutdown_waiting, self()})
          receive do
            :release -> exit(reason)
          after
            5_000 -> raise "held shutdown never released"
          end
        else
          exit(reason)
        end
    end
  end
end

defmodule SupervisorSupplement do
  def log(), do: Agent.start_link(fn -> [] end)
  def take(log), do: Agent.get_and_update(log, fn events -> {Enum.join(events, ","), []} end)
  def child(name, log, opts \\ []) do
    %{id: name,
      start: {SupervisorSupplement.Worker, :start_link,
              [{name, log, self(), Keyword.get(opts, :mode, :ready)}]},
      restart: Keyword.get(opts, :restart, :permanent),
      shutdown: Keyword.get(opts, :shutdown, 5_000),
      significant: Keyword.get(opts, :significant, false)}
  end
  def snapshot(sup), do: Map.new(Supervisor.which_children(sup), fn {id, pid, _, _} -> {id, pid} end)
  def wait_for(fun), do: wait_for(fun, System.monotonic_time(:millisecond) + 5_000)
  def wait_for(fun, deadline) do
    case fun.() do
      false ->
        if System.monotonic_time(:millisecond) >= deadline, do: raise("state never converged")
        :erlang.yield()
        wait_for(fun, deadline)
      result -> result
    end
  end
  def down(ref, pid) do
    receive do
      {:DOWN, ^ref, :process, ^pid, reason} -> reason
    after
      5_000 -> raise "retirement acknowledgement missing"
    end
  end
  def finish(sup, log) do
    if Process.alive?(sup), do: Supervisor.stop(sup)
    Agent.stop(log)
  end

  def policies do
    for restart <- [:permanent, :transient, :temporary], reason <- [:normal, :shutdown, {:shutdown, :detail}, :broken] do
      {:ok, log} = log()
      {:ok, sup} = Supervisor.start_link([child(:a, log, restart: restart)],
                                        strategy: :one_for_one, max_restarts: 10)
      old = snapshot(sup).a
      ref = Process.monitor(old)
      send(old, {:finish, reason})
      ^reason = down(ref, old)
      state = cond do
        restart == :permanent or (restart == :transient and reason == :broken) ->
          wait_for(fn ->
            new = Map.get(snapshot(sup), :a)
            is_pid(new) and new != old
          end)
          "fresh"
        restart == :temporary ->
          wait_for(fn -> not Map.has_key?(snapshot(sup), :a) end)
          "removed"
        true ->
          wait_for(fn -> Map.get(snapshot(sup), :a) == :undefined end)
          "stopped"
      end
      label = if is_tuple(reason), do: "shutdown_detail", else: Atom.to_string(reason)
      IO.puts("#{restart}_#{label}=#{state}")
      finish(sup, log)
    end
  end

  def startup do
    {:ok, log} = log()
    result = Supervisor.start_link([
      child(:a, log), child(:b, log), child(:c, log, mode: {:fail, :broken}), child(:d, log)
    ], strategy: :one_for_one)
    {:error, {:shutdown, {:failed_to_start_child, :c, :broken}}} = result
    IO.puts("startup_failure=#{take(log)}")
    Agent.stop(log)

    {:ok, log} = log()
    {:ok, sup} = Supervisor.start_link([
      child(:p, log, mode: :ignore),
      child(:t, log, mode: :ignore, restart: :transient),
      child(:x, log, mode: :ignore, restart: :temporary)
    ], strategy: :one_for_one)
    IO.puts("ignore_specs=#{snapshot(sup) == %{p: :undefined, t: :undefined}}")
    finish(sup, log)
  end

  def failed_restarts do
    {:ok, log} = log()
    {:ok, script} = Agent.start_link(fn -> [:ready, {:fail, :broken}] end)
    {:ok, sup} = Supervisor.start_link([child(:a, log, mode: {:script, script})],
                  strategy: :one_for_one, max_restarts: 2, max_seconds: 3_600)
    ref = Process.monitor(sup)
    old = snapshot(sup).a
    take(log)
    send(old, {:finish, :broken})
    :shutdown = down(ref, sup)
    IO.puts("failed_restart_attempts=#{take(log)}")
    Agent.stop(script)
    Agent.stop(log)
  end

  def mixed_group do
    {:ok, log} = log()
    {:ok, sup} = Supervisor.start_link([
      child(:a, log), child(:b, log, restart: :temporary), child(:c, log, restart: :transient)
    ], strategy: :one_for_all, max_restarts: 10)
    old = snapshot(sup)
    take(log)
    send(old.a, {:finish, :broken})
    wait_for(fn ->
      state = snapshot(sup)
      is_pid(state[:a]) and state[:a] != old.a and is_pid(state[:c]) and state[:c] != old.c
    end)
    IO.puts("mixed_group=#{take(log)}")
    IO.puts("mixed_temporary_removed=#{not Map.has_key?(snapshot(sup), :b)}")
    finish(sup, log)
  end

  def group_retry(strategy) do
    {:ok, log} = log()
    {:ok, script} = Agent.start_link(fn -> [:ready, {:fail, :broken}, :ready] end)
    {:ok, sup} = Supervisor.start_link([
      child(:a, log), child(:b, log), child(:c, log, mode: {:script, script}), child(:d, log)
    ], strategy: strategy, max_restarts: 10, max_seconds: 3_600)
    old = snapshot(sup)
    take(log)
    send(old.b, {:finish, :broken})
    wait_for(fn ->
      state = snapshot(sup)
      is_pid(state[:d]) and state[:d] != old.d
    end)
    IO.puts("#{strategy}_failed_initializer=#{take(log)}")
    finish(sup, log)
    Agent.stop(script)
  end

  def manual_zero_and_immediate_deadline do
    {:ok, log} = log()
    {:ok, sup} = Supervisor.start_link([child(:a, log)],
                           strategy: :one_for_one, max_restarts: 0)
    first = snapshot(sup).a
    last = Enum.reduce(1..3, first, fn _, previous ->
      :ok = Supervisor.terminate_child(sup, :a)
      {:ok, next} = Supervisor.restart_child(sup, :a)
      if next == previous, do: raise("manual restart reused identity")
      next
    end)
    IO.puts("manual_zero_fresh=#{last != first}")
    ref = Process.monitor(sup)
    send(last, {:finish, :broken})
    IO.puts("manual_zero_exit=#{down(ref, sup)}")
    Agent.stop(log)

    {:ok, log} = log()
    {:ok, sup} = Supervisor.start_link([child(:a, log, mode: :hold, shutdown: 0)],
                                      strategy: :one_for_one)
    pid = snapshot(sup).a
    ref = Process.monitor(pid)
    :ok = Supervisor.terminate_child(sup, :a)
    IO.puts("graceful_zero=#{down(ref, pid)}")
    finish(sup, log)
  end

  def significant do
    {:ok, log} = log()
    {:ok, sup} = Supervisor.start_link([
      child(:a, log, restart: :temporary, significant: true), child(:other, log)
    ], strategy: :one_for_one, auto_shutdown: :any_significant)
    ref = Process.monitor(sup)
    a = snapshot(sup).a
    take(log)
    send(a, {:finish, :broken})
    :shutdown = down(ref, sup)
    IO.puts("any_significant=#{take(log)}")
    Agent.stop(log)

    {:ok, log} = log()
    {:ok, sup} = Supervisor.start_link([
      child(:a, log, restart: :transient, significant: true),
      child(:b, log, restart: :transient, significant: true)
    ], strategy: :one_for_one, auto_shutdown: :all_significant)
    ref = Process.monitor(sup)
    :ok = Supervisor.terminate_child(sup, :a)
    IO.puts("all_manual_keeps_alive=#{Process.alive?(sup)}")
    b = snapshot(sup).b
    send(b, {:finish, :normal})
    IO.puts("all_last_natural=#{down(ref, sup) == :shutdown}")
    Agent.stop(log)

    {:ok, log} = log()
    {:ok, sup} = Supervisor.start_link([
      child(:a, log, restart: :transient, significant: true)
    ], strategy: :one_for_one, auto_shutdown: :any_significant, max_restarts: 10)
    old = snapshot(sup).a
    send(old, {:finish, :broken})
    wait_for(fn -> new = snapshot(sup).a; is_pid(new) and new != old end)
    IO.puts("significant_abnormal_restarts=#{Process.alive?(sup)}")
    finish(sup, log)

    {:ok, sup} = Supervisor.start_link([], strategy: :one_for_one, auto_shutdown: :all_significant)
    IO.puts("empty_significant_keeps_alive=#{Process.alive?(sup)}")
    Supervisor.stop(sup)

    {:ok, log} = log()
    permanent = Supervisor.start_link([child(:a, log, significant: true)],
                         strategy: :one_for_one, auto_shutdown: :any_significant)
    never = Supervisor.start_link([child(:a, log, restart: :temporary, significant: true)],
                         strategy: :one_for_one, auto_shutdown: :never)
    IO.puts("invalid_significant=#{match?({:error, _}, permanent) and match?({:error, _}, never)}")
    IO.puts("invalid_significant_no_body=#{take(log) == ""}")
    Agent.stop(log)
  end

  def infinity_and_unlink do
    {:ok, log} = log()
    {:ok, sup} = Supervisor.start_link([child(:a, log, mode: :hold, shutdown: :infinity)],
                                      strategy: :one_for_one)
    pid = snapshot(sup).a
    send(pid, {:unlink, self()})
    receive do
      {:unlinked, ^pid} -> :ok
    after
      5_000 -> raise "unlink not acknowledged"
    end
    owner = self()
    stop_ref = make_ref()
    spawn(fn -> send(owner, {stop_ref, Supervisor.stop(sup)}) end)
    receive do
      {:shutdown_waiting, ^pid} -> :ok
    after
      5_000 -> raise "shutdown not delivered after unlink"
    end
    IO.puts("infinity_waits_after_unlink=#{Process.alive?(sup) and Process.alive?(pid)}")
    send(pid, :release)
    receive do
      {^stop_ref, :ok} -> IO.puts("infinity_release_stops=true")
    after
      5_000 -> raise "stop did not finish after release"
    end
    Agent.stop(log)
  end

  def nested_finite_branch do
    {:ok, log} = log()
    leaf_spec = child(:leaf, log, mode: :hold, shutdown: :infinity)
    branch_spec = %{id: :branch,
      start: {Supervisor, :start_link, [[leaf_spec], [strategy: :one_for_one]]},
      restart: :permanent, shutdown: 0, type: :supervisor}
    {:ok, outer} = Supervisor.start_link([branch_spec], strategy: :one_for_one)
    branch = snapshot(outer).branch
    leaf = snapshot(branch).leaf
    branch_ref = Process.monitor(branch)
    leaf_ref = Process.monitor(leaf)
    owner = self()
    stop_ref = make_ref()
    spawn(fn ->
      result = try do
        {:ok, Supervisor.stop(branch)}
      catch
        :exit, reason -> {:exit, reason}
      end
      send(owner, {stop_ref, result})
    end)
    # Establish that branch shutdown is already blocked on a live descendant.
    receive do
      {:shutdown_waiting, ^leaf} -> :ok
    after
      5_000 -> raise "nested descendant never entered shutdown"
    end
    :ok = Supervisor.stop(outer)
    IO.puts("nested_finite_branch=#{down(branch_ref, branch)}")
    # Branch retirement is not proof that a trapping descendant has retired.
    IO.puts("nested_descendant_still_waiting=#{Process.alive?(leaf)}")
    receive do
      {^stop_ref, {:exit, _}} -> :ok
    after
      5_000 -> raise "interrupted branch stop did not return"
    end
    send(leaf, :release)
    IO.puts("nested_descendant_release=#{down(leaf_ref, leaf)}")
    Agent.stop(log)
  end

  def run do
    Logger.configure(level: :emergency)
    Process.flag(:trap_exit, true)
    policies()
    startup()
    failed_restarts()
    mixed_group()
    group_retry(:one_for_all)
    group_retry(:rest_for_one)
    manual_zero_and_immediate_deadline()
    significant()
    infinity_and_unlink()
    nested_finite_branch()
  end
end

SupervisorSupplement.run()
