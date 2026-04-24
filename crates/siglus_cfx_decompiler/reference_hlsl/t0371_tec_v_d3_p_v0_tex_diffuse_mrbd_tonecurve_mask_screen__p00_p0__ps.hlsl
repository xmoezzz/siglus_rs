uniform sampler2D s0;
uniform sampler2D s1;
uniform sampler2D s2;

static const float4 c3 = float4(-2.0, 1.0, 0.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
    float4 t1 : TEXCOORD1;
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
    float4 r4 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r5 = float4(0.0, 0.0, 0.0, 0.0);

    r0 = tex2D(s0, input.t0.xy);
    r1 = (r0 * input.v0);
    r0.w = dot(c0, r1);
    r0.xyz = (input.v0.xyz * -(r0.xyz) + r0.www);
    r2.xyz = (c2.yyy * r0.xyz + r1.xyz);
    r2.w = c2.x;
    r0.x = r2.x;
    r0.y = r2.w;
    r2.x = r2.y;
    r2.y = r2.w;
    r3.x = r2.z;
    r3.y = r2.w;
    r4 = tex2D(s2, r0.xy);
    r2 = tex2D(s2, r2.xy);
    r3 = tex2D(s2, r3.xy);
    r5 = tex2D(s1, input.t1.xy);
    r3.x = r4.x;
    r3.y = r2.y;
    r0.xyz = (r3.xyz * c3.xxx + c3.yyy);
    r0.xyz = (c1.yyy * r0.xyz + r3.xyz);
    r2.xyz = lerp(r0.xyz, r0.www, c1.xxx);
    r0.xyz = (r2.xyz + c1.zzz);
    r0.xyz = (r0.xyz + -(c1.www));
    r1.xyz = (r1.www * r0.xyz);
    r0 = (r5 * r1);
    output.oC0 = r0;
    return output;
}
