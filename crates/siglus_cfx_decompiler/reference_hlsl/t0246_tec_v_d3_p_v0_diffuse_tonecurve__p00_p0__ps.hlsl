uniform sampler2D s0;

struct PS_INPUT {
    float4 v0 : COLOR0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = input.v0.w;
    r1.w = dot(c0, input.v0);
    r2.xyz = lerp(input.v0.xyz, r1.www, c1.yyy);
    r2.w = c1.x;
    r1.x = r2.x;
    r1.y = r2.w;
    r2.x = r2.y;
    r2.y = r2.w;
    r3.x = r2.z;
    r3.y = r2.w;
    r1 = tex2D(s0, r1.xy);
    r2 = tex2D(s0, r2.xy);
    r3 = tex2D(s0, r3.xy);
    r0.x = r1.x;
    r0.y = r2.y;
    r0.z = r3.z;
    output.oC0 = r0;
    return output;
}
