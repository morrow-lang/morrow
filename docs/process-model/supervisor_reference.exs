# Pinned OTP oracle for local supervisor policies. No sleeps or timing assertions.
defmodule SupervisorReference.Worker do
  def start_link({name, log, parent}) do
    caller = self()
    pid = spawn_link(fn ->
      Process.flag(:trap_exit, true)
      Agent.update(log, &(&1 ++ ["start:#{name}"]))
      send(caller, {:ready, self()})
      send(parent, {:started, name, self()})
      loop(name, log)
    end)
    receive do
      {:ready, ^pid} -> {:ok, pid}
    after
      5_000 -> raise "startup acknowledgement missing"
    end
  end

  def loop(name, log) do
    receive do
      {:finish, reason} -> exit(reason)
      {:EXIT, _, reason} ->
        Agent.update(log, &(&1 ++ ["stop:#{name}"]))
        exit(reason)
    end
  end
end

defmodule SupervisorReference do
  def started(name) do
    receive do
      {:started, ^name, pid} -> pid
    after
      5_000 -> raise "missing start #{name}"
    end
  end

  def child(name, log, restart \\ :permanent) do
    %{id: name, start: {SupervisorReference.Worker, :start_link, [{name, log, self()}]},
      restart: restart, shutdown: 5_000}
  end

  def take_log(log), do: Agent.get_and_update(log, fn events -> {Enum.join(events, ","), []} end)

  def strategy(strategy) do
    {:ok, log} = Agent.start_link(fn -> [] end)
    children = Enum.map([:a, :b, :c, :d], &child(&1, log))
    {:ok, sup} = Supervisor.start_link(children, strategy: strategy, max_restarts: 10)
    pids = Map.new([:a, :b, :c, :d], &{&1, started(&1)})
    IO.puts("#{strategy}_startup=#{take_log(log)}")
    send(pids.b, {:finish, :shutdown})
    restarted = case strategy do
      :one_for_one -> [:b]
      :one_for_all -> [:a, :b, :c, :d]
      :rest_for_one -> [:b, :c, :d]
    end
    fresh = Enum.all?(restarted, fn name -> started(name) != pids[name] end)
    IO.puts("#{strategy}_restart=#{take_log(log)}")
    IO.puts("#{strategy}_fresh=#{fresh}")
    :ok = Supervisor.stop(sup)
    IO.puts("#{strategy}_shutdown=#{take_log(log)}")
    Agent.stop(log)
  end

  def policies do
    {:ok, log} = Agent.start_link(fn -> [] end)
    {:ok, sup} = Supervisor.start_link([
      child(:permanent, log), child(:transient, log, :transient), child(:temporary, log, :temporary)
    ], strategy: :one_for_one, max_restarts: 10)
    original = Map.new([:permanent, :transient, :temporary], &{&1, started(&1)})
    take_log(log)
    :ok = Supervisor.terminate_child(sup, :permanent)
    IO.puts("manual_stop=#{take_log(log)}")
    {:ok, replacement} = Supervisor.restart_child(sup, :permanent)
    ^replacement = started(:permanent)
    IO.puts("manual_restart_fresh=#{replacement != original.permanent}")
    take_log(log)
    :ok = Supervisor.terminate_child(sup, :transient)
    :ok = Supervisor.delete_child(sup, :transient)
    IO.puts("deleted_restart=#{inspect(Supervisor.restart_child(sup, :transient))}")
    :ok = Supervisor.terminate_child(sup, :temporary)
    IO.puts("temporary_restart=#{inspect(Supervisor.restart_child(sup, :temporary))}")
    :ok = Supervisor.stop(sup)
    Agent.stop(log)
  end

  def run do
    # The expected child exits are policy input; suppress OTP's diagnostic logger
    # while retaining every explicit observation and failing watchdog exception.
    Logger.configure(level: :emergency)
    Enum.each([:one_for_one, :one_for_all, :rest_for_one], &strategy/1)
    policies()
  end
end

SupervisorReference.run()
